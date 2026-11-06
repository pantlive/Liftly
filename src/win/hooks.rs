//! 低级键盘/鼠标钩子。休息期间吞掉所有输入，并处理紧急解锁组合键。
//!
//! 钩子回调运行在主线程的消息循环中，但必须保持极短：回调里只操作原子量，
//! 复杂逻辑通过 PostMessage 交给主窗口。

use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, Ordering};

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, VK_CONTROL, VK_F12, VK_LCONTROL, VK_LMENU, VK_LSHIFT, VK_MENU, VK_RCONTROL,
    VK_RMENU, VK_RSHIFT, VK_SHIFT,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, PostMessageW, SetWindowsHookExW, UnhookWindowsHookEx, HHOOK, KBDLLHOOKSTRUCT,
    WH_KEYBOARD_LL, WH_MOUSE_LL, WM_KEYDOWN, WM_KEYUP, WM_SYSKEYDOWN, WM_SYSKEYUP,
};

use super::WM_EMERGENCY_UNLOCK;

static LOCKED: AtomicBool = AtomicBool::new(false);
static EMERGENCY_ENABLED: AtomicBool = AtomicBool::new(false);
static MAIN_HWND: AtomicIsize = AtomicIsize::new(0);
static MODIFIERS: AtomicU32 = AtomicU32::new(0);
static EMERGENCY_SENT: AtomicBool = AtomicBool::new(false);

const CTRL_LEFT: u32 = 1 << 0;
const CTRL_RIGHT: u32 = 1 << 1;
const CTRL_GENERIC: u32 = 1 << 2;
const ALT_LEFT: u32 = 1 << 3;
const ALT_RIGHT: u32 = 1 << 4;
const ALT_GENERIC: u32 = 1 << 5;
const SHIFT_LEFT: u32 = 1 << 6;
const SHIFT_RIGHT: u32 = 1 << 7;
const SHIFT_GENERIC: u32 = 1 << 8;

const CTRL_BITS: u32 = CTRL_LEFT | CTRL_RIGHT | CTRL_GENERIC;
const ALT_BITS: u32 = ALT_LEFT | ALT_RIGHT | ALT_GENERIC;
const SHIFT_BITS: u32 = SHIFT_LEFT | SHIFT_RIGHT | SHIFT_GENERIC;

pub struct Hooks {
    keyboard: HHOOK,
    mouse: HHOOK,
}

pub fn install(main_hwnd: HWND, emergency_unlock: bool) -> Hooks {
    unsafe {
        LOCKED.store(true, Ordering::SeqCst);
        EMERGENCY_ENABLED.store(emergency_unlock, Ordering::SeqCst);
        MAIN_HWND.store(main_hwnd as isize, Ordering::SeqCst);
        MODIFIERS.store(initial_modifiers(), Ordering::SeqCst);
        EMERGENCY_SENT.store(false, Ordering::SeqCst);

        let module = GetModuleHandleW(std::ptr::null());
        let keyboard = SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), module, 0);
        let mouse = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), module, 0);
        Hooks { keyboard, mouse }
    }
}

/// 续装钩子：系统可能在回调超时后静默卸载 LL 钩子，因此休息期间定期重装。
/// 不重置 MODIFIERS / EMERGENCY_SENT / LOCKED 等运行状态。
pub fn reinstall(hooks: &mut Hooks) {
    unsafe {
        let module = GetModuleHandleW(std::ptr::null());
        let keyboard = SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), module, 0);
        let mouse = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), module, 0);

        if !keyboard.is_null() {
            if !hooks.keyboard.is_null() {
                // 旧句柄可能已经被系统卸载，失败可忽略。
                UnhookWindowsHookEx(hooks.keyboard);
            }
            hooks.keyboard = keyboard;
        }
        if !mouse.is_null() {
            if !hooks.mouse.is_null() {
                UnhookWindowsHookEx(hooks.mouse);
            }
            hooks.mouse = mouse;
        }
    }
}

pub fn uninstall(hooks: Hooks) {
    unsafe {
        // 先停止吞输入，再卸载钩子；即使某个钩子安装失败也能安全处理。
        LOCKED.store(false, Ordering::SeqCst);
        if !hooks.keyboard.is_null() {
            UnhookWindowsHookEx(hooks.keyboard);
        }
        if !hooks.mouse.is_null() {
            UnhookWindowsHookEx(hooks.mouse);
        }
        MAIN_HWND.store(0, Ordering::SeqCst);
        EMERGENCY_ENABLED.store(false, Ordering::SeqCst);
        MODIFIERS.store(0, Ordering::SeqCst);
        EMERGENCY_SENT.store(false, Ordering::SeqCst);
    }
}

unsafe extern "system" fn keyboard_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code < 0 || !LOCKED.load(Ordering::Relaxed) {
        return CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam);
    }
    if lparam == 0 {
        return 1;
    }

    let info = &*(lparam as *const KBDLLHOOKSTRUCT);
    let vk = info.vkCode as u16;
    let message = wparam as u32;
    let is_down = matches!(message, WM_KEYDOWN | WM_SYSKEYDOWN);
    let is_up = matches!(message, WM_KEYUP | WM_SYSKEYUP);

    match (modifier_bit(vk), is_down, is_up) {
        (Some(bit), true, _) => {
            MODIFIERS.fetch_or(bit, Ordering::Relaxed);
        }
        (Some(bit), _, true) => {
            // 抬起任一 Ctrl/Alt/Shift 类键时，同时清除该键的位和对应的 GENERIC 位。
            let mask = bit | modifier_generic(vk);
            MODIFIERS.fetch_and(!mask, Ordering::Relaxed);
        }
        _ => {}
    }

    if is_down && vk == VK_F12 && EMERGENCY_ENABLED.load(Ordering::SeqCst) {
        let modifiers = MODIFIERS.load(Ordering::SeqCst);
        if modifiers & CTRL_BITS != 0
            && modifiers & ALT_BITS != 0
            && modifiers & SHIFT_BITS != 0
            && !EMERGENCY_SENT.swap(true, Ordering::SeqCst)
        {
            let hwnd = MAIN_HWND.load(Ordering::SeqCst);
            if hwnd != 0 {
                PostMessageW(hwnd as HWND, WM_EMERGENCY_UNLOCK, 0, 0);
            }
        }
    }

    // 锁定期间吞掉所有键盘输入。
    1
}

unsafe extern "system" fn mouse_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code < 0 || !LOCKED.load(Ordering::Relaxed) {
        CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam)
    } else {
        1
    }
}

fn modifier_bit(vk: u16) -> Option<u32> {
    match vk {
        VK_LCONTROL => Some(CTRL_LEFT),
        VK_RCONTROL => Some(CTRL_RIGHT),
        VK_CONTROL => Some(CTRL_GENERIC),
        VK_LMENU => Some(ALT_LEFT),
        VK_RMENU => Some(ALT_RIGHT),
        VK_MENU => Some(ALT_GENERIC),
        VK_LSHIFT => Some(SHIFT_LEFT),
        VK_RSHIFT => Some(SHIFT_RIGHT),
        VK_SHIFT => Some(SHIFT_GENERIC),
        _ => None,
    }
}

fn modifier_generic(vk: u16) -> u32 {
    match vk {
        VK_LCONTROL | VK_RCONTROL | VK_CONTROL => CTRL_GENERIC,
        VK_LMENU | VK_RMENU | VK_MENU => ALT_GENERIC,
        VK_LSHIFT | VK_RSHIFT | VK_SHIFT => SHIFT_GENERIC,
        _ => 0,
    }
}

/// 进入休息时，把已经按住的修饰键也纳入识别，避免钩子安装前按下的 Ctrl/Alt/Shift 丢失。
fn initial_modifiers() -> u32 {
    let mut modifiers = 0;
    unsafe {
        if GetAsyncKeyState(i32::from(VK_CONTROL)) as u16 & 0x8000 != 0 {
            modifiers |= CTRL_GENERIC;
        }
        if GetAsyncKeyState(i32::from(VK_MENU)) as u16 & 0x8000 != 0 {
            modifiers |= ALT_GENERIC;
        }
        if GetAsyncKeyState(i32::from(VK_SHIFT)) as u16 & 0x8000 != 0 {
            modifiers |= SHIFT_GENERIC;
        }
    }
    modifiers
}
