//! 托盘图标、气泡通知和右键菜单。

use std::mem::size_of;
use std::ptr::null;

use windows_sys::Win32::Foundation::{HWND, POINT};
use windows_sys::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_TIP, NIIF_INFO, NIM_ADD, NIM_DELETE,
    NIM_MODIFY, NOTIFYICONDATAW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, DestroyMenu, GetCursorPos, LoadIconW, PostMessageW,
    SetForegroundWindow, TrackPopupMenu, IDI_APPLICATION, IDI_INFORMATION, MF_SEPARATOR, MF_STRING,
    TPM_NONOTIFY, TPM_RETURNCMD, TPM_RIGHTBUTTON, WM_NULL,
};

use super::{wide, WM_TRAY_CALLBACK};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuAction {
    BreakNow,
    TogglePause,
    Restart,
    Exit,
}

const ID_BREAK_NOW: usize = 1;
const ID_TOGGLE_PAUSE: usize = 2;
const ID_RESTART: usize = 3;
const ID_EXIT: usize = 5;

pub struct Tray {
    nid: NOTIFYICONDATAW,
    added: bool,
    last_tip: String,
}

impl Tray {
    pub fn new(hwnd: HWND, tip: &str) -> Self {
        let mut icon = unsafe { LoadIconW(std::ptr::null_mut(), IDI_INFORMATION) };
        if icon.is_null() {
            icon = unsafe { LoadIconW(std::ptr::null_mut(), IDI_APPLICATION) };
        }

        let mut nid: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
        nid.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
        nid.hWnd = hwnd;
        nid.uID = 1;
        nid.uFlags = base_flags();
        nid.uCallbackMessage = WM_TRAY_CALLBACK;
        nid.hIcon = icon;
        copy_wide(&mut nid.szTip, tip);

        let mut tray = Self {
            nid,
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

    pub fn balloon(&mut self, title: &str, text: &str) {
        if !self.added {
            return;
        }
        copy_wide(&mut self.nid.szInfoTitle, title);
        copy_wide(&mut self.nid.szInfo, text);
        self.nid.dwInfoFlags = NIIF_INFO;
        self.nid.uFlags = NIF_INFO;
        let ok = unsafe { Shell_NotifyIconW(NIM_MODIFY, &self.nid) } != 0;
        self.nid.uFlags = base_flags();
        if !ok {
            self.added = false;
        }
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
pub fn show_menu(hwnd: HWND, paused: bool) -> Option<MenuAction> {
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
            ID_EXIT => Some(MenuAction::Exit),
            _ => None,
        }
    }
}
