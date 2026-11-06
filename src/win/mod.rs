//! Windows 专用实现：单实例、隐藏消息窗口、托盘、提醒弹窗、计时驱动与休息锁定。

pub mod hooks;
pub mod overlay;
pub mod popup;
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

use crate::config::Config;
use crate::schedule::{Event, Phase, Schedule};

const MUTEX_NAME: &str = "Local\\Liftly.SingleInstance";
const WINDOW_CLASS: &str = "Liftly.HiddenWindow";
const TIMER_ID: usize = 1;

pub const WM_TRAY_CALLBACK: u32 = WM_APP + 1;
pub const WM_EMERGENCY_UNLOCK: u32 = WM_APP + 2;

static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);
static MENU_OPEN: AtomicBool = AtomicBool::new(false);

thread_local! {
    static APP: RefCell<Option<Box<App>>> = const { RefCell::new(None) };
}

/// UTF-16 字符串辅助函数，返回值以 0 结尾。
pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// 程序入口。
pub fn run() {
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
        register_main_class(hinstance);

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
            hooks: None,
            wts_registered: false,
            away: false,
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
                app.arm_timer(now);
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

fn load_config() -> Config {
    let ini = std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(|dir| dir.join("liftly.ini")))
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

unsafe fn register_main_class(hinstance: windows_sys::Win32::Foundation::HINSTANCE) {
    let class = wide(WINDOW_CLASS);
    let wc = WNDCLASSW {
        style: 0,
        lpfnWndProc: Some(main_wnd_proc),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: hinstance,
        hIcon: LoadIconW(std::ptr::null_mut(), IDI_INFORMATION),
        hCursor: LoadCursorW(std::ptr::null_mut(), IDC_ARROW),
        hbrBackground: std::ptr::null_mut(),
        lpszMenuName: std::ptr::null(),
        lpszClassName: class.as_ptr(),
    };
    RegisterClassW(&wc);
}

fn now_ms() -> u64 {
    unsafe { GetTickCount64() }
}

enum WndOutcome {
    Handled(LRESULT),
    Default,
    ShowMenu,
    DestroyMain,
}

struct App {
    hwnd: HWND,
    cfg: Config,
    schedule: Schedule,
    tray: tray::Tray,
    overlay: overlay::Overlay,
    popup: popup::Popup,
    hooks: Option<hooks::Hooks>,
    wts_registered: bool,
    away: bool,
}

impl App {
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

    fn arm_timer(&mut self, now: u64) {
        let delay = self.schedule.next_wakeup_ms(now);
        unsafe {
            SetTimer(self.hwnd, TIMER_ID, delay, None);
        }
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
        self.arm_timer(now);
    }

    fn on_display_change(&mut self) {
        if self.schedule.phase() == Phase::Breaking {
            let deadline = self.schedule.deadline_ms();
            self.overlay.rebuild(deadline, self.cfg.emergency_unlock);
        }
    }

    fn stop_break_resources(&mut self) {
        if let Some(hooks) = self.hooks.take() {
            hooks::uninstall(hooks);
        }
        self.overlay.stop();
    }

    fn cleanup(&mut self) {
        unsafe {
            KillTimer(self.hwnd, TIMER_ID);
        }
        self.stop_break_resources();
        self.popup.hide();
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
                }
                WndOutcome::Handled(0)
            }
            WM_TRAY_CALLBACK => {
                let mouse_msg = lparam as u32;
                if mouse_msg == WM_RBUTTONUP || mouse_msg == WM_CONTEXTMENU {
                    WndOutcome::ShowMenu
                } else {
                    WndOutcome::Handled(0)
                }
            }
            WM_EMERGENCY_UNLOCK => {
                self.emergency_unlock();
                WndOutcome::Handled(0)
            }
            WM_DISPLAYCHANGE => {
                self.on_display_change();
                WndOutcome::Handled(0)
            }
            WM_WTSSESSION_CHANGE => {
                if self.cfg.reset_on_unlock {
                    match wparam as u32 {
                        // 只在正常工作时响应锁屏；手动暂停或休息中保持原状态。
                        WTS_SESSION_LOCK if self.schedule.phase() == Phase::Working => {
                            let now = now_ms();
                            self.schedule.pause(now);
                            self.away = true;
                            self.popup.hide();
                            self.update_tip(now);
                            self.arm_timer(now);
                        }
                        WTS_SESSION_UNLOCK if self.away => {
                            let now = now_ms();
                            self.schedule.reset_work(now);
                            self.away = false;
                            self.popup.hide();
                            self.update_tip(now);
                            self.arm_timer(now);
                        }
                        _ => {}
                    }
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
        WndOutcome::DestroyMain => {
            DestroyWindow(hwnd);
            0
        }
    }
}

/// 弹出托盘菜单。菜单期间不暂停定时器：TrackPopupMenu 的模态循环会分发 WM_TIMER，
/// 若计时到点会在关闭菜单后进入休息。
unsafe fn show_tray_menu(hwnd: HWND) {
    let paused = APP.with(|cell| match cell.try_borrow() {
        Ok(slot) => slot
            .as_ref()
            .map(|app| app.schedule.phase() == Phase::Paused)
            .unwrap_or(false),
        Err(_) => false,
    });

    MENU_OPEN.store(true, Ordering::SeqCst);

    let action = tray::show_menu(hwnd, paused);
    MENU_OPEN.store(false, Ordering::SeqCst);
    let mut destroy = false;
    let mut acted = false;

    if let Some(action) = action {
        destroy = APP.with(|cell| match cell.try_borrow_mut() {
            Ok(mut slot) => match slot.as_mut() {
                Some(app) => {
                    acted = true;
                    match action {
                        tray::MenuAction::BreakNow => {
                            app.break_now();
                            false
                        }
                        tray::MenuAction::TogglePause => {
                            app.toggle_pause();
                            false
                        }
                        tray::MenuAction::Restart => {
                            app.restart_work();
                            false
                        }
                        tray::MenuAction::Exit => {
                            app.cleanup();
                            true
                        }
                    }
                }
                None => false,
            },
            Err(_) => false,
        });
    }

    if destroy {
        DestroyWindow(hwnd);
        return;
    }

    // 菜单取消或普通操作后，重新装一个定时器。
    let now = now_ms();
    APP.with(|cell| {
        if let Ok(mut slot) = cell.try_borrow_mut() {
            if let Some(app) = slot.as_mut() {
                if !acted {
                    app.update_tip(now);
                }
                app.arm_timer(now);
            }
        }
    });
}
