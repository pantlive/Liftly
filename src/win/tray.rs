//! 托盘图标、气泡通知和右键菜单。

use std::mem::size_of;
use std::ptr::null;

use windows_sys::Win32::Foundation::{HWND, POINT};
use windows_sys::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_TIP, NIIF_INFO, NIM_ADD, NIM_DELETE,
    NIM_MODIFY, NOTIFYICONDATAW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, DestroyMenu, GetCursorPos, GetSystemMetrics, PostMessageW,
    SetForegroundWindow, TrackPopupMenu, HMENU, MF_CHECKED, MF_POPUP, MF_SEPARATOR, MF_STRING,
    SM_CXSMICON, SM_CYSMICON, TPM_NONOTIFY, TPM_RETURNCMD, TPM_RIGHTBUTTON, WM_NULL,
};

use super::{wide, AppIcon, WM_TRAY_CALLBACK};
use crate::config::{Config, BREAK_PRESETS, WARN_PRESETS, WORK_PRESETS};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuAction {
    BreakNow,
    TogglePause,
    Restart,
    ToggleBubble,
    SetWork(u32),
    SetBreak(u32),
    SetWarn(u32),
    OpenSettings,
    Exit,
}

const ID_BREAK_NOW: usize = 1;
const ID_TOGGLE_PAUSE: usize = 2;
const ID_RESTART: usize = 3;
const ID_SETTINGS: usize = 4;
const ID_EXIT: usize = 5;
const ID_TOGGLE_BUBBLE: usize = 6;

pub struct Tray {
    nid: NOTIFYICONDATAW,
    _icon: AppIcon,
    added: bool,
    last_tip: String,
}

impl Tray {
    pub fn new(hwnd: HWND, tip: &str) -> Self {
        // 图标持有至托盘移除之后；Explorer 重启时复用同一有效句柄。
        let icon =
            unsafe { AppIcon::load(GetSystemMetrics(SM_CXSMICON), GetSystemMetrics(SM_CYSMICON)) };

        let mut nid: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
        nid.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
        nid.hWnd = hwnd;
        nid.uID = 1;
        nid.uFlags = base_flags();
        nid.uCallbackMessage = WM_TRAY_CALLBACK;
        nid.hIcon = icon.handle;
        copy_wide(&mut nid.szTip, tip);

        let mut tray = Self {
            nid,
            _icon: icon,
            added: false,
            last_tip: tip.to_owned(),
        };
        tray.add();
        tray
    }

    fn add(&mut self) {
        self.nid.uFlags = base_flags();
        self.added = unsafe { Shell_NotifyIconW(NIM_ADD, &self.nid) != 0 };
    }

    /// 托盘图标当前是否已成功添加；启动阶段据此决定是否重试。
    pub fn is_added(&self) -> bool {
        self.added
    }

    /// Explorer 重启后重新添加图标。
    pub fn readd(&mut self) {
        self.added = false;
        self.add();
    }

    /// 仅在提示文字变化时调用 NIM_MODIFY。
    pub fn update_tip(&mut self, tip: &str) {
        if self.last_tip == tip {
            return;
        }
        copy_wide(&mut self.nid.szTip, tip);
        self.last_tip.clear();
        self.last_tip.push_str(tip);
        if !self.added {
            return;
        }

        self.nid.uFlags = NIF_TIP;
        let ok = unsafe { Shell_NotifyIconW(NIM_MODIFY, &self.nid) } != 0;
        self.nid.uFlags = base_flags();
        if !ok {
            self.added = false;
        }
    }

    /// 返回气泡是否提交成功。通知失败不代表图标已丢失，不能据此清除 `added`；
    /// 是否重试由调用方决定。
    pub fn balloon(&mut self, title: &str, text: &str) -> bool {
        if !self.added {
            return false;
        }
        copy_wide(&mut self.nid.szInfoTitle, title);
        copy_wide(&mut self.nid.szInfo, text);
        self.nid.dwInfoFlags = NIIF_INFO;
        self.nid.uFlags = NIF_INFO;
        let ok = unsafe { Shell_NotifyIconW(NIM_MODIFY, &self.nid) } != 0;
        self.nid.uFlags = base_flags();
        ok
    }

    pub fn remove(&mut self) {
        if self.added {
            unsafe {
                Shell_NotifyIconW(NIM_DELETE, &self.nid);
            }
            self.added = false;
        }
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        self.remove();
    }
}

fn base_flags() -> u32 {
    NIF_ICON | NIF_MESSAGE | NIF_TIP
}

fn copy_wide(dst: &mut [u16], text: &str) {
    if dst.is_empty() {
        return;
    }
    let mut index = 0;
    for unit in text.encode_utf16() {
        if index + 1 >= dst.len() {
            break;
        }
        dst[index] = unit;
        index += 1;
    }
    dst[index] = 0;
}

/// 在鼠标位置弹出右键菜单；菜单期间的输入由系统模态循环处理。
pub fn show_menu(hwnd: HWND, paused: bool, cfg: &Config) -> Option<MenuAction> {
    unsafe {
        let menu = CreatePopupMenu();
        if menu.is_null() {
            return None;
        }

        let break_text = wide("立即休息");
        let pause_text = wide(if paused {
            "继续计时"
        } else {
            "暂停计时"
        });
        let restart_text = wide("重新开始计时");
        let exit_text = wide("退出");

        AppendMenuW(menu, MF_STRING, ID_BREAK_NOW, break_text.as_ptr());
        AppendMenuW(menu, MF_STRING, ID_TOGGLE_PAUSE, pause_text.as_ptr());
        AppendMenuW(menu, MF_STRING, ID_RESTART, restart_text.as_ptr());
        let bubble_flags = MF_STRING | if cfg.show_bubble { MF_CHECKED } else { 0 };
        AppendMenuW(
            menu,
            bubble_flags,
            ID_TOGGLE_BUBBLE,
            wide("显示桌面倒计时").as_ptr(),
        );
        AppendMenuW(menu, MF_SEPARATOR, 0, null());
        // 子菜单挂到父菜单后由父菜单统一释放，失败时只销毁尚未挂接的句柄。
        for (label, presets, current, base, unit) in [
            (
                "工作时长",
                WORK_PRESETS.as_slice(),
                cfg.work_minutes,
                100,
                "分钟",
            ),
            (
                "休息时长",
                BREAK_PRESETS.as_slice(),
                cfg.break_minutes,
                200,
                "分钟",
            ),
            (
                "锁定前提醒",
                WARN_PRESETS.as_slice(),
                cfg.warn_seconds,
                300,
                "秒",
            ),
        ] {
            if !append_presets(menu, label, presets, current, base, unit) {
                DestroyMenu(menu);
                return None;
            }
        }
        AppendMenuW(menu, MF_STRING, ID_SETTINGS, wide("设置…").as_ptr());
        AppendMenuW(menu, MF_SEPARATOR, 0, null());
        AppendMenuW(menu, MF_STRING, ID_EXIT, exit_text.as_ptr());

        let mut point: POINT = std::mem::zeroed();
        GetCursorPos(&mut point);
        SetForegroundWindow(hwnd);

        let command = TrackPopupMenu(
            menu,
            TPM_RIGHTBUTTON | TPM_RETURNCMD | TPM_NONOTIFY,
            point.x,
            point.y,
            0,
            hwnd,
            null(),
        );

        // 标准做法：菜单关闭后给窗口投递一个空消息，修复前台窗口状态。
        PostMessageW(hwnd, WM_NULL, 0, 0);
        DestroyMenu(menu);

        match command as usize {
            ID_BREAK_NOW => Some(MenuAction::BreakNow),
            ID_TOGGLE_PAUSE => Some(MenuAction::TogglePause),
            ID_RESTART => Some(MenuAction::Restart),
            ID_TOGGLE_BUBBLE => Some(MenuAction::ToggleBubble),
            ID_SETTINGS | 199 | 299 | 399 => Some(MenuAction::OpenSettings),
            ID_EXIT => Some(MenuAction::Exit),
            id if (100..100 + WORK_PRESETS.len()).contains(&id) => {
                Some(MenuAction::SetWork(WORK_PRESETS[id - 100]))
            }
            id if (200..200 + BREAK_PRESETS.len()).contains(&id) => {
                Some(MenuAction::SetBreak(BREAK_PRESETS[id - 200]))
            }
            id if (300..300 + WARN_PRESETS.len()).contains(&id) => {
                Some(MenuAction::SetWarn(WARN_PRESETS[id - 300]))
            }
            _ => None,
        }
    }
}

/// 创建预设子菜单，自定义值显示在末项；父菜单持有成功挂接的子菜单。
unsafe fn append_presets(
    parent: HMENU,
    label: &str,
    presets: &[u32],
    current: u32,
    base: usize,
    unit: &str,
) -> bool {
    let menu = CreatePopupMenu();
    if menu.is_null() {
        return false;
    }
    for (index, value) in presets.iter().enumerate() {
        let text = if *value == 0 {
            "不提醒".to_owned()
        } else {
            format!("{value} {unit}")
        };
        let flags = MF_STRING | if *value == current { MF_CHECKED } else { 0 };
        if AppendMenuW(menu, flags, base + index, wide(&text).as_ptr()) == 0 {
            DestroyMenu(menu);
            return false;
        }
    }
    let custom = !presets.contains(&current);
    let text = if custom {
        format!("自定义（{current} {unit}）")
    } else {
        "自定义…".to_owned()
    };
    if AppendMenuW(
        menu,
        MF_STRING | if custom { MF_CHECKED } else { 0 },
        base + 99,
        wide(&text).as_ptr(),
    ) == 0
        || AppendMenuW(parent, MF_POPUP, menu as usize, wide(label).as_ptr()) == 0
    {
        DestroyMenu(menu);
        return false;
    }
    true
}
