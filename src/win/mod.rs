//! Windows 专用实现：单实例、隐藏消息窗口、托盘、提醒弹窗、计时驱动与休息锁定。

pub mod bubble;
pub mod hooks;
pub mod overlay;
pub mod popup;
pub mod settings;
pub mod tray;

use std::cell::RefCell;
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use windows_sys::Win32::Foundation::{
    GetLastError, SetLastError, BOOL, ERROR_ALREADY_EXISTS, HANDLE, HWND, LPARAM, LRESULT, WPARAM,
};
use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows_sys::Win32::System::RemoteDesktop::{
    WTSRegisterSessionNotification, WTSUnRegisterSessionNotification, NOTIFY_FOR_THIS_SESSION,
};
use windows_sys::Win32::System::SystemInformation::GetTickCount64;
use windows_sys::Win32::System::Threading::CreateMutexW;
use windows_sys::Win32::UI::HiDpi::DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

use crate::config::{merge_into_ini, BubblePosition, Config, ConfigPatch};
use crate::schedule::{Event, Phase, Schedule};
use crate::view;

const MUTEX_NAME: &str = "Local\\Liftly.SingleInstance";
const WINDOW_CLASS: &str = "Liftly.HiddenWindow";
const TIMER_ID: usize = 1;
const STARTUP_TIMER_ID: usize = 2;
const STARTUP_RETRY_LIMIT: u32 = 10;

pub const WM_TRAY_CALLBACK: u32 = WM_APP + 1;
pub const WM_EMERGENCY_UNLOCK: u32 = WM_APP + 2;
pub const WM_BUBBLE_MOVED: u32 = WM_APP + 3;

static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);
static MENU_OPEN: AtomicBool = AtomicBool::new(false);

thread_local! {
    static APP: RefCell<Option<Box<App>>> = const { RefCell::new(None) };
}

/// UTF-16 字符串辅助函数，返回值以 0 结尾。
pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn message_box(text: &str, flags: u32) {
    unsafe {
        let title = wide("站会儿");
        let text = wide(text);
        MessageBoxW(std::ptr::null_mut(), text.as_ptr(), title.as_ptr(), flags);
    }
}

/// GUI 子系统没有控制台；panic 时至少给用户一个可见的中文提示。
fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        message_box(
            "程序内部错误，即将退出。请重新启动站会儿；若问题持续出现，请查看 README 的常见问题。",
            MB_OK | MB_ICONERROR,
        );
        previous(info);
    }));
}

/// 程序入口。
pub fn run() {
    install_panic_hook();

    unsafe {
        if is_second_instance() {
            let text = wide("站会儿已经在运行");
            let title = wide("站会儿");
            MessageBoxW(
                std::ptr::null_mut(),
                text.as_ptr(),
                title.as_ptr(),
                MB_OK | MB_ICONINFORMATION,
            );
            return;
        }

        set_dpi_awareness();

        let cfg = load_config();
        let hinstance = GetModuleHandleW(std::ptr::null());
        // 图标必须活到窗口类注销之后，失败时由 None 提前结束启动。
        let Some(_main_icons) = register_main_class(hinstance) else {
            return;
        };

        let class = wide(WINDOW_CLASS);
        let title = wide("站会儿");
        let hwnd = CreateWindowExW(
            0,
            class.as_ptr(),
            title.as_ptr(),
            WS_OVERLAPPED,
            0,
            0,
            0,
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            hinstance,
            std::ptr::null(),
        );
        if hwnd.is_null() {
            let code = GetLastError();
            message_box(
                &format!("创建主窗口失败（错误码 {code}），程序无法启动。"),
                MB_OK | MB_ICONERROR,
            );
            UnregisterClassW(class.as_ptr(), hinstance);
            return;
        }

        let taskbar_created = RegisterWindowMessageW(wide("TaskbarCreated").as_ptr());
        TASKBAR_CREATED.store(taskbar_created, Ordering::Relaxed);

        let now = now_ms();
        let schedule = Schedule::new(&cfg, now);
        let app = App {
            hwnd,
            cfg,
            schedule,
            tray: tray::Tray::new(hwnd, "站会儿"),
            overlay: overlay::Overlay::new(),
            popup: popup::Popup::new(),
            bubble: bubble::Bubble::new(hwnd),
            hooks: None,
            wts_registered: false,
            away: false,
            locked: false,
            startup_attempts: 0,
        };
        APP.with(|cell| {
            *cell.borrow_mut() = Some(Box::new(app));
        });

        // 会话解锁通知；失败不影响主功能。
        let wts_ok = WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION) != 0;
        APP.with(|cell| {
            if let Some(app) = cell.borrow_mut().as_mut() {
                app.wts_registered = wts_ok;
                app.update_tip(now);
                app.sync_bubble(now);
                app.arm_timer(now);
                // 托盘图标和启动气泡等主循环开始后再处理，避免启动阶段弹窗阻塞初始化。
                SetTimer(hwnd, STARTUP_TIMER_ID, 500, None);
            }
        });

        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        // 正常退出时的兜底清理；cleanup 本身可重复调用。
        APP.with(|cell| {
            let mut slot = cell.borrow_mut();
            if let Some(app) = slot.as_mut() {
                app.cleanup();
            }
            slot.take();
        });
        // 先注销窗口类，再由局部变量释放该类引用的图标。
        UnregisterClassW(class.as_ptr(), hinstance);
    }
}

unsafe fn is_second_instance() -> bool {
    SetLastError(0);
    let handle: HANDLE = CreateMutexW(std::ptr::null(), 0, wide(MUTEX_NAME).as_ptr());
    if handle.is_null() {
        return false;
    }
    if GetLastError() == ERROR_ALREADY_EXISTS {
        windows_sys::Win32::Foundation::CloseHandle(handle);
        true
    } else {
        // 故意不关闭：句柄随进程退出释放，用于维持单实例。
        false
    }
}

unsafe fn set_dpi_awareness() {
    let user32 = GetModuleHandleW(wide("user32.dll").as_ptr());
    if !user32.is_null() {
        let proc_name = b"SetProcessDpiAwarenessContext\0";
        if let Some(proc) = GetProcAddress(user32, proc_name.as_ptr()) {
            type SetProcessDpiAwarenessContext = unsafe extern "system" fn(*mut c_void) -> BOOL;
            let set_context: SetProcessDpiAwarenessContext = std::mem::transmute(proc);
            if set_context(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) != 0 {
                return;
            }
        }
    }
    let _ = SetProcessDPIAware();
}

fn ini_path() -> std::io::Result<std::path::PathBuf> {
    let exe = std::env::current_exe()?;
    let dir = exe
        .parent()
        .ok_or_else(|| std::io::Error::other("无法确定程序目录"))?;
    Ok(dir.join("liftly.ini"))
}

fn load_config() -> Config {
    let ini = ini_path()
        .ok()
        .and_then(|path| std::fs::read_to_string(path).ok());
    let mut cfg = match ini {
        Some(text) => Config::parse_ini(&text),
        None => Config::default(),
    };
    cfg.apply_args(
        std::env::args_os()
            .skip(1)
            .filter_map(|arg| arg.into_string().ok()),
    );
    cfg
}

/// 自有图标按需释放；系统兜底图标为共享句柄，不销毁。
struct AppIcon {
    handle: HICON,
    owned: bool,
}

impl AppIcon {
    fn load(width: i32, height: i32) -> Self {
        unsafe {
            let handle = LoadImageW(
                GetModuleHandleW(std::ptr::null()),
                std::ptr::without_provenance::<u16>(1), // MAKEINTRESOURCEW(1)，组图标资源 ID。
                IMAGE_ICON,
                width,
                height,
                0,
            ) as HICON;
            if handle.is_null() {
                Self {
                    handle: LoadIconW(std::ptr::null_mut(), IDI_APPLICATION),
                    owned: false,
                }
            } else {
                Self {
                    handle,
                    owned: true,
                }
            }
        }
    }
}

impl Drop for AppIcon {
    fn drop(&mut self) {
        if self.owned {
            unsafe {
                DestroyIcon(self.handle);
            }
        }
    }
}

/// 注册成功时返回持有图标的数组；失败时显示错误码并返回 None。
unsafe fn register_main_class(
    hinstance: windows_sys::Win32::Foundation::HINSTANCE,
) -> Option<[AppIcon; 2]> {
    let class = wide(WINDOW_CLASS);
    // 分别按系统尺寸加载大小图标，避免共享缓存返回错误尺寸。
    let icons = [
        AppIcon::load(GetSystemMetrics(SM_CXICON), GetSystemMetrics(SM_CYICON)),
        AppIcon::load(GetSystemMetrics(SM_CXSMICON), GetSystemMetrics(SM_CYSMICON)),
    ];
    let wc = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        style: 0,
        lpfnWndProc: Some(main_wnd_proc),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: hinstance,
        hIcon: icons[0].handle,
        hCursor: LoadCursorW(std::ptr::null_mut(), IDC_ARROW),
        hbrBackground: std::ptr::null_mut(),
        lpszMenuName: std::ptr::null(),
        lpszClassName: class.as_ptr(),
        hIconSm: icons[1].handle,
    };
    if RegisterClassExW(&wc) == 0 {
        let code = GetLastError();
        message_box(
            &format!("注册主窗口类失败（错误码 {code}），程序无法启动。"),
            MB_OK | MB_ICONERROR,
        );
        return None;
    }
    Some(icons)
}

fn now_ms() -> u64 {
    unsafe { GetTickCount64() }
}

enum WndOutcome {
    Handled(LRESULT),
    Default,
    ShowMenu,
    OpenSettings,
    DestroyMain,
}

struct App {
    hwnd: HWND,
    cfg: Config,
    schedule: Schedule,
    tray: tray::Tray,
    overlay: overlay::Overlay,
    popup: popup::Popup,
    bubble: bubble::Bubble,
    hooks: Option<hooks::Hooks>,
    wts_registered: bool,
    away: bool,
    locked: bool,
    startup_attempts: u32,
}

impl App {
    /// 所有设置入口共用同一流程，写盘失败仍保留本次运行的新配置。
    fn apply_patch(&mut self, patch: ConfigPatch) {
        let Ok(cfg) = self.cfg.apply_patch(&patch) else {
            return;
        };
        self.cfg = cfg;
        let now = now_ms();
        self.schedule.apply_config(&self.cfg, now);
        self.popup.hide();
        self.sync_bubble(now);
        self.update_tip(now);
        self.arm_timer(now);
        self.save_patch(&patch);
    }

    /// 只把本次用户修改的字段写回 INI；保存失败不影响本次运行。
    fn save_patch(&mut self, patch: &ConfigPatch) {
        let saved = (|| -> Result<(), String> {
            let path = ini_path().map_err(|error| error.to_string())?;
            let original = match std::fs::read_to_string(&path) {
                Ok(text) => text,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
                Err(error) => return Err(error.to_string()),
            };
            let text = merge_into_ini(&original, patch)?;
            std::fs::write(path, text).map_err(|error| error.to_string())
        })();
        if saved.is_err() {
            self.tray
                .balloon("站会儿", "已生效，但没能保存到 liftly.ini");
        }
    }

    /// 气泡开关只影响显示和 1 秒刷新节奏，不重置计时，也不关闭提醒弹窗。
    fn set_bubble_enabled(&mut self, enabled: bool) {
        if self.cfg.show_bubble == enabled {
            return;
        }
        let now = now_ms();
        self.cfg.show_bubble = enabled;
        if enabled {
            self.bubble.reset_failure();
        }
        self.sync_bubble(now);
        self.update_tip(now);
        self.arm_timer(now);
        self.save_patch(&ConfigPatch {
            show_bubble: Some(enabled),
            ..ConfigPatch::default()
        });
    }

    /// 气泡大小只影响窗口尺寸和绘制缩放，不重置计时。
    fn set_bubble_scale(&mut self, percent: u32) {
        if self.cfg.bubble_scale == percent {
            return;
        }
        let now = now_ms();
        self.cfg.bubble_scale = percent;
        self.sync_bubble(now);
        self.save_patch(&ConfigPatch {
            bubble_scale: Some(percent),
            ..ConfigPatch::default()
        });
    }

    fn update_tip(&mut self, now: u64) {
        let tip = match self.schedule.phase() {
            Phase::Working if self.schedule.warning_active(now) => {
                let secs = self.schedule.remaining_ms(now).div_ceil(1_000);
                format!("站会儿：{} 秒后休息", secs)
            }
            Phase::Working => {
                let minutes = self.schedule.remaining_ms(now).saturating_add(59_999) / 60_000;
                format!("站会儿：距离休息还有 {} 分钟", minutes.max(1))
            }
            Phase::Paused => "站会儿：已暂停".to_owned(),
            Phase::Breaking => "站会儿：休息中".to_owned(),
        };
        self.tray.update_tip(&tip);
    }

    fn bubble_should_show(&self) -> bool {
        view::bubble_visible(self.schedule.phase(), self.cfg.show_bubble, self.locked)
    }

    /// 工作且气泡可用时每秒刷新，暂停时保持静态文本。
    fn sync_bubble(&mut self, now: u64) {
        if !self.bubble_should_show() {
            self.bubble.hide();
            return;
        }
        if self.bubble.failed() {
            return;
        }
        let remaining = self.schedule.remaining_ms(now);
        let paused = self.schedule.phase() == Phase::Paused;
        let saved = self.cfg.bubble_position.coordinates();
        // 创建失败时静默降级，核心计时和托盘功能不受影响。
        self.bubble
            .show(remaining, paused, saved, self.cfg.bubble_scale);
    }

    /// 拖动结束或显示器变化后，把窗口实际左上角写回 INI；不走 apply_patch，避免重置计时。
    fn save_bubble_position(&mut self, x: i32, y: i32) {
        if self.cfg.bubble_position == BubblePosition::At(x, y) {
            return;
        }
        self.cfg.bubble_position = BubblePosition::At(x, y);
        self.save_patch(&ConfigPatch {
            bubble_position: Some(BubblePosition::At(x, y)),
            ..ConfigPatch::default()
        });
    }

    fn arm_timer(&mut self, now: u64) {
        let mut delay = self.schedule.next_wakeup_ms(now);
        if self.schedule.phase() == Phase::Working
            && self.bubble_should_show()
            && !self.bubble.failed()
        {
            delay = delay.min(1_000);
        }
        unsafe {
            SetTimer(self.hwnd, TIMER_ID, delay, None);
        }
    }

    /// 托盘缺失时重试添加，成功后显示一次带当前时长的启动气泡。
    /// 返回 false 表示启动失败，调用方应销毁主窗口退出。
    fn announce_startup(&mut self) -> bool {
        if !self.tray.is_added() {
            self.tray.readd();
            if !self.tray.is_added() {
                self.startup_attempts = self.startup_attempts.saturating_add(1);
                if self.startup_attempts >= STARTUP_RETRY_LIMIT {
                    unsafe {
                        KillTimer(self.hwnd, STARTUP_TIMER_ID);
                    }
                    message_box(
                        "无法创建托盘图标，程序不能继续运行。\n请确认任务栏通知区域可用后重新启动站会儿。",
                        MB_OK | MB_ICONERROR,
                    );
                    return false;
                }
                unsafe {
                    SetTimer(self.hwnd, STARTUP_TIMER_ID, 1_000, None);
                }
                return true;
            }
        }

        unsafe {
            KillTimer(self.hwnd, STARTUP_TIMER_ID);
        }
        let text = format!(
            "已开始计时：工作 {} 分钟，休息 {} 分钟。\n右键托盘图标可设置，双击打开设置。",
            self.cfg.work_minutes, self.cfg.break_minutes
        );
        if !self.tray.balloon("站会儿已启动", &text) {
            message_box(
                "启动提示气泡显示失败，但程序已经在托盘中运行。\n请检查任务栏通知区域设置。",
                MB_OK | MB_ICONWARNING,
            );
        }
        true
    }

    fn on_timer(&mut self) {
        let now = now_ms();
        let events = self.schedule.tick(now);

        for event in events {
            match event {
                Event::Warn { secs_left } => {
                    if !self.popup.show(self.schedule.deadline_ms()) {
                        // 弹窗创建失败时保留原来的托盘气泡提醒。
                        let text =
                            format!("还有 {} 秒就要站起来活动啦，请保存手头工作。", secs_left);
                        self.tray.balloon("站会儿", &text);
                    }
                }
                Event::StartBreak => {
                    // 菜单的模态循环会分发 WM_TIMER；进入休息前先关闭菜单。
                    if MENU_OPEN.load(Ordering::SeqCst) {
                        unsafe {
                            EndMenu();
                        }
                    }
                    settings::close_without_save();
                    self.enter_break(now);
                }
                Event::EndBreak => {
                    self.stop_break_resources();
                    self.tray.balloon("站会儿", "休息结束，继续加油！");
                }
            }
        }

        if self.schedule.warning_active(now) {
            if self.popup.is_visible() {
                self.popup.refresh();
            }
        } else if self.popup.is_visible() {
            self.popup.hide();
        }

        if self.schedule.phase() == Phase::Breaking {
            // LL 钩子可能被系统静默卸载，休息期间随每秒刷新一起续装。
            if let Some(hooks) = self.hooks.as_mut() {
                hooks::reinstall(hooks);
            }
            self.overlay.refresh();
        }
        self.update_tip(now);
        self.sync_bubble(now);
        self.arm_timer(now);
    }

    fn enter_break(&mut self, now: u64) {
        self.popup.hide();
        let deadline = self.schedule.deadline_ms();
        self.overlay.start(deadline, self.cfg.emergency_unlock);
        if self.hooks.is_none() {
            self.hooks = Some(hooks::install(self.hwnd, self.cfg.emergency_unlock));
        }
        self.update_tip(now);
        self.sync_bubble(now);
    }

    fn break_now(&mut self) {
        // 定时器可能已经在菜单打开期间进入了休息，此时不要重置休息截止时间。
        if self.schedule.phase() == Phase::Breaking {
            return;
        }
        let now = now_ms();
        self.schedule.start_break_now(now);
        self.enter_break(now);
        self.arm_timer(now);
    }

    fn toggle_pause(&mut self) {
        // 用户通过菜单操作说明已经回到前台，不再处于锁屏离开状态。
        self.away = false;
        let now = now_ms();
        match self.schedule.phase() {
            Phase::Working => self.schedule.pause(now),
            Phase::Paused => self.schedule.resume(now),
            Phase::Breaking => return,
        }
        self.popup.hide();
        self.update_tip(now);
        self.sync_bubble(now);
        self.arm_timer(now);
    }

    fn restart_work(&mut self) {
        if self.schedule.phase() == Phase::Breaking {
            return;
        }
        let now = now_ms();
        self.schedule.reset_work(now);
        self.popup.hide();
        self.update_tip(now);
        self.sync_bubble(now);
        self.arm_timer(now);
    }

    fn emergency_unlock(&mut self) {
        if !self.cfg.emergency_unlock || self.schedule.phase() != Phase::Breaking {
            return;
        }
        let now = now_ms();
        self.schedule.end_break_now(now);
        self.stop_break_resources();
        self.tray.balloon("站会儿", "休息结束，继续加油！");
        self.update_tip(now);
        self.sync_bubble(now);
        self.arm_timer(now);
    }

    fn on_display_change(&mut self) {
        if self.schedule.phase() == Phase::Breaking {
            let deadline = self.schedule.deadline_ms();
            self.overlay.rebuild(deadline, self.cfg.emergency_unlock);
        }
        self.sync_bubble(now_ms());
        // 显示器拔插或缩放变化后，尺寸和位置可能越界；同步时不会移动已有窗口。
        self.bubble.reclamp();
    }

    fn stop_break_resources(&mut self) {
        if let Some(hooks) = self.hooks.take() {
            hooks::uninstall(hooks);
        }
        self.overlay.stop();
    }

    fn cleanup(&mut self) {
        settings::close_without_save();
        unsafe {
            KillTimer(self.hwnd, TIMER_ID);
            KillTimer(self.hwnd, STARTUP_TIMER_ID);
        }
        self.stop_break_resources();
        self.popup.hide();
        self.bubble.hide();
        self.tray.remove();
        if self.wts_registered {
            unsafe {
                WTSUnRegisterSessionNotification(self.hwnd);
            }
            self.wts_registered = false;
        }
    }

    fn handle_message(&mut self, msg: u32, wparam: WPARAM, lparam: LPARAM) -> WndOutcome {
        if msg == TASKBAR_CREATED.load(Ordering::Relaxed) && msg != 0 {
            self.tray.readd();
            self.update_tip(now_ms());
            return WndOutcome::Handled(0);
        }

        match msg {
            WM_TIMER => {
                // 菜单打开期间也照常处理：TrackPopupMenu 的模态循环会分发 WM_TIMER。
                if wparam == TIMER_ID {
                    self.on_timer();
                } else if wparam == STARTUP_TIMER_ID && !self.announce_startup() {
                    return WndOutcome::DestroyMain;
                }
                WndOutcome::Handled(0)
            }
            WM_TRAY_CALLBACK => {
                let mouse_msg = lparam as u32;
                if mouse_msg == WM_RBUTTONUP || mouse_msg == WM_CONTEXTMENU {
                    WndOutcome::ShowMenu
                } else if mouse_msg == WM_LBUTTONDBLCLK {
                    WndOutcome::OpenSettings
                } else {
                    WndOutcome::Handled(0)
                }
            }
            WM_EMERGENCY_UNLOCK => {
                self.emergency_unlock();
                WndOutcome::Handled(0)
            }
            WM_BUBBLE_MOVED => {
                self.save_bubble_position(wparam as i32, lparam as i32);
                WndOutcome::Handled(0)
            }
            WM_DISPLAYCHANGE => {
                self.on_display_change();
                WndOutcome::Handled(0)
            }
            WM_SETTINGCHANGE if wparam == SPI_SETWORKAREA as usize => {
                // 任务栏位置或占用区域改变时，分辨率可能不变，仍需重新夹取。
                self.bubble.reclamp();
                WndOutcome::Handled(0)
            }
            WM_WTSSESSION_CHANGE => {
                let now = now_ms();
                match wparam as u32 {
                    WTS_SESSION_LOCK if !self.locked => {
                        self.locked = true;
                        // 锁屏始终隐藏气泡；是否暂停计时仍由 reset_on_unlock 决定。
                        if self.cfg.reset_on_unlock && self.schedule.phase() == Phase::Working {
                            self.schedule.pause(now);
                            self.away = true;
                            self.popup.hide();
                            self.update_tip(now);
                        }
                        self.sync_bubble(now);
                        self.arm_timer(now);
                    }
                    WTS_SESSION_UNLOCK if self.locked => {
                        self.locked = false;
                        if self.cfg.reset_on_unlock && self.away {
                            self.schedule.reset_work(now);
                            self.away = false;
                            self.popup.hide();
                            self.update_tip(now);
                        }
                        self.sync_bubble(now);
                        self.arm_timer(now);
                    }
                    _ => {}
                }
                WndOutcome::Handled(0)
            }
            WM_POWERBROADCAST => {
                let event = wparam as u32;
                // 唤醒时若会话仍处于锁屏离开状态（away），保持暂停，等 WTS_SESSION_UNLOCK 再重置；
                // 只有正常工作时才直接重置，避免清掉 away 后在锁屏背后继续计时。
                if self.cfg.reset_on_unlock
                    && (event == PBT_APMRESUMEAUTOMATIC || event == PBT_APMRESUMESUSPEND)
                    && !self.away
                    && self.schedule.phase() == Phase::Working
                {
                    let now = now_ms();
                    self.schedule.reset_work(now);
                    self.popup.hide();
                    self.update_tip(now);
                    self.sync_bubble(now);
                    self.arm_timer(now);
                }
                WndOutcome::Handled(1)
            }
            WM_QUERYENDSESSION => WndOutcome::Handled(1),
            WM_ENDSESSION => {
                if wparam != 0 {
                    self.cleanup();
                    unsafe {
                        PostQuitMessage(0);
                    }
                }
                WndOutcome::Handled(0)
            }
            WM_CLOSE => {
                self.cleanup();
                WndOutcome::DestroyMain
            }
            WM_DESTROY => {
                self.cleanup();
                unsafe {
                    PostQuitMessage(0);
                }
                WndOutcome::Handled(0)
            }
            _ => WndOutcome::Default,
        }
    }
}

unsafe extern "system" fn main_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let outcome = APP.with(|cell| match cell.try_borrow_mut() {
        Ok(mut slot) => match slot.as_mut() {
            Some(app) => app.handle_message(msg, wparam, lparam),
            None => WndOutcome::Default,
        },
        Err(_) => WndOutcome::Default,
    });

    match outcome {
        WndOutcome::Handled(value) => value,
        WndOutcome::Default => DefWindowProcW(hwnd, msg, wparam, lparam),
        WndOutcome::ShowMenu => {
            show_tray_menu(hwnd);
            0
        }
        WndOutcome::OpenSettings => {
            open_settings(hwnd);
            0
        }
        WndOutcome::DestroyMain => {
            DestroyWindow(hwnd);
            0
        }
    }
}

/// 弹出托盘菜单。菜单期间不暂停定时器：TrackPopupMenu 的模态循环会分发 WM_TIMER，
/// 若计时到点会在关闭菜单后进入休息。
unsafe fn show_tray_menu(hwnd: HWND) {
    if settings::bring_to_front() || MENU_OPEN.load(Ordering::SeqCst) {
        return;
    }
    let state = APP.with(|cell| {
        let slot = cell.try_borrow().ok()?;
        let app = slot.as_ref()?;
        (app.schedule.phase() != Phase::Breaking)
            .then(|| (app.schedule.phase() == Phase::Paused, app.cfg.clone()))
    });
    let Some((paused, cfg)) = state else {
        return;
    };
    MENU_OPEN.store(true, Ordering::SeqCst);
    let action = tray::show_menu(hwnd, paused, &cfg);
    MENU_OPEN.store(false, Ordering::SeqCst);

    // 任何模态窗口都必须在释放 APP 借用后打开，确保 WM_TIMER 能重入。
    if action == Some(tray::MenuAction::OpenSettings) {
        open_settings(hwnd);
        return;
    }
    let destroy = APP.with(|cell| {
        let Ok(mut slot) = cell.try_borrow_mut() else {
            return false;
        };
        let Some(app) = slot.as_mut() else {
            return false;
        };
        match action {
            Some(tray::MenuAction::BreakNow) => app.break_now(),
            Some(tray::MenuAction::TogglePause) => app.toggle_pause(),
            Some(tray::MenuAction::Restart) => app.restart_work(),
            Some(tray::MenuAction::ToggleBubble) => {
                let enabled = !app.cfg.show_bubble;
                app.set_bubble_enabled(enabled);
            }
            Some(tray::MenuAction::SetBubbleScale(value)) => app.set_bubble_scale(value),
            Some(tray::MenuAction::SetWork(value)) => app.apply_patch(ConfigPatch {
                work_minutes: Some(value),
                ..ConfigPatch::default()
            }),
            Some(tray::MenuAction::SetBreak(value)) => app.apply_patch(ConfigPatch {
                break_minutes: Some(value),
                ..ConfigPatch::default()
            }),
            Some(tray::MenuAction::SetWarn(value)) => app.apply_patch(ConfigPatch {
                warn_seconds: Some(value),
                ..ConfigPatch::default()
            }),
            Some(tray::MenuAction::Exit) => {
                app.cleanup();
                return true;
            }
            _ => {}
        }
        app.update_tip(now_ms());
        app.arm_timer(now_ms());
        false
    });
    if destroy {
        DestroyWindow(hwnd);
    }
}

/// 系统对话框自带模态消息循环；调用前后均只短暂借用 APP。
unsafe fn open_settings(hwnd: HWND) {
    if settings::bring_to_front() || MENU_OPEN.load(Ordering::SeqCst) {
        return;
    }
    let cfg = APP.with(|cell| {
        let slot = cell.try_borrow().ok()?;
        let app = slot.as_ref()?;
        (app.schedule.phase() != Phase::Breaking).then(|| app.cfg.clone())
    });
    let Some(cfg) = cfg else {
        return;
    };
    let patch = settings::show(hwnd, &cfg);
    APP.with(|cell| {
        if let Ok(mut slot) = cell.try_borrow_mut() {
            if let Some(app) = slot.as_mut() {
                if let Some(patch) = patch {
                    app.apply_patch(patch);
                }
                app.arm_timer(now_ms());
            }
        }
    });
}
