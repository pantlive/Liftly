//! 原生模态设置对话框，使用内存模板获得系统的键盘导航及 DPI 字体缩放。

use std::ffi::c_void;
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicPtr, Ordering};

use windows_sys::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Controls::{BST_CHECKED, EM_SETLIMITTEXT};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

use super::{autostart, wide};
use crate::config::{Config, ConfigPatch};

const ID_WORK: i32 = 1001;
const ID_BREAK: i32 = 1002;
const ID_WARN: i32 = 1003;
const ID_RESET: i32 = 1004;
const ID_EMERGENCY: i32 = 1005;
const ID_AUTOSTART: i32 = 1006;

static SETTINGS_HWND: AtomicPtr<c_void> = AtomicPtr::new(null_mut());

struct DialogState {
    initial: Config,
    autostart_initial: bool,
    patch: Option<ConfigPatch>,
}

/// 已有窗口时只将其激活，避免模态对话框嵌套。
pub fn bring_to_front() -> bool {
    let hwnd = SETTINGS_HWND.load(Ordering::SeqCst);
    if hwnd.is_null() {
        return false;
    }
    unsafe {
        // 校验提示存在时激活它，避免把已禁用的设置窗口置前。
        let popup = GetLastActivePopup(hwnd);
        SetForegroundWindow(if popup.is_null() { hwnd } else { popup });
    }
    true
}

/// 休息到点时关闭设置和可能打开的校验提示，不保存输入。
pub fn close_without_save() {
    let hwnd = SETTINGS_HWND.load(Ordering::SeqCst);
    if !hwnd.is_null() {
        unsafe {
            let popup = GetLastActivePopup(hwnd);
            if !popup.is_null() && popup != hwnd {
                PostMessageW(popup, WM_CLOSE, 0, 0);
            }
            EndDialog(hwnd, IDCANCEL as isize);
        }
    }
}

/// 对话框返回后模板及状态才释放，系统消息回调期间地址保持有效。
pub fn show(owner: HWND, cfg: &Config) -> Option<ConfigPatch> {
    if bring_to_front() {
        return None;
    }
    let template = dialog_template();
    let mut state = DialogState {
        initial: cfg.clone(),
        autostart_initial: autostart::is_enabled(),
        patch: None,
    };
    let result = unsafe {
        DialogBoxIndirectParamW(
            GetModuleHandleW(null()),
            template.as_ptr().cast(),
            owner,
            Some(dialog_proc),
            (&mut state as *mut DialogState) as LPARAM,
        )
    };
    if result == IDOK as isize {
        state.patch
    } else {
        None
    }
}

unsafe extern "system" fn dialog_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> isize {
    match msg {
        WM_INITDIALOG => {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, lparam);
            SETTINGS_HWND.store(hwnd, Ordering::SeqCst);
            let state = &*(lparam as *const DialogState);
            for (id, value) in [
                (ID_WORK, state.initial.work_minutes),
                (ID_BREAK, state.initial.break_minutes),
                (ID_WARN, state.initial.warn_seconds),
            ] {
                SetDlgItemInt(hwnd, id, value, 0);
                // 限制输入长度，防止超长文本被截断后误认为合法值。
                SendDlgItemMessageW(hwnd, id, EM_SETLIMITTEXT, 10, 0);
            }
            for (id, checked) in [
                (ID_RESET, state.initial.reset_on_unlock),
                (ID_EMERGENCY, state.initial.emergency_unlock),
                (ID_AUTOSTART, state.autostart_initial),
            ] {
                SendDlgItemMessageW(hwnd, id, BM_SETCHECK, usize::from(checked), 0);
            }
            1
        }
        WM_COMMAND => {
            match (wparam & 0xffff) as i32 {
                IDOK => {
                    let state_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut DialogState;
                    if state_ptr.is_null() {
                        return 1;
                    }
                    // 校验提示也会运行消息循环，弹出前不持有状态的可变引用。
                    match read_patch(hwnd, &(*state_ptr).initial) {
                        Ok(patch) => {
                            let checked =
                                SendDlgItemMessageW(hwnd, ID_AUTOSTART, BM_GETCHECK, 0, 0)
                                    == BST_CHECKED as isize;
                            let autostart_ok = if checked != (*state_ptr).autostart_initial {
                                match autostart::set_enabled(checked) {
                                    Ok(()) => true,
                                    Err(message) => {
                                        MessageBoxW(
                                            hwnd,
                                            wide(&format!("开机自启动设置失败：{message}"))
                                                .as_ptr(),
                                            wide("设置提示").as_ptr(),
                                            MB_OK | MB_ICONWARNING,
                                        );
                                        false
                                    }
                                }
                            } else {
                                true
                            };
                            if autostart_ok {
                                (*state_ptr).patch =
                                    (patch != ConfigPatch::default()).then_some(patch);
                                EndDialog(hwnd, IDOK as isize);
                            }
                        }
                        Err(message) => {
                            MessageBoxW(
                                hwnd,
                                wide(&message).as_ptr(),
                                wide("设置提示").as_ptr(),
                                MB_OK | MB_ICONWARNING,
                            );
                        }
                    }
                }
                IDCANCEL => {
                    EndDialog(hwnd, IDCANCEL as isize);
                }
                _ => return 0,
            }
            1
        }
        WM_CLOSE => {
            EndDialog(hwnd, IDCANCEL as isize);
            1
        }
        WM_NCDESTROY => {
            SETTINGS_HWND
                .compare_exchange(hwnd, null_mut(), Ordering::SeqCst, Ordering::SeqCst)
                .ok();
            0
        }
        _ => 0,
    }
}

/// 只保存相对打开窗口时发生变化的字段，保留命令行及磁盘其他配置。
unsafe fn read_patch(hwnd: HWND, initial: &Config) -> Result<ConfigPatch, String> {
    let mut values = [0; 3];
    for (index, id) in [ID_WORK, ID_BREAK, ID_WARN].into_iter().enumerate() {
        let mut text = [0_u16; 32];
        let len = GetDlgItemTextW(hwnd, id, text.as_mut_ptr(), text.len() as i32);
        values[index] = String::from_utf16_lossy(&text[..len as usize])
            .parse::<u32>()
            .map_err(|_| "请输入数字".to_owned())?;
    }
    // 使用配置层统一的范围检查；提醒秒数的归一化在实际应用补丁时执行。
    initial.with_times(values[0], values[1], values[2])?;
    let reset = SendDlgItemMessageW(hwnd, ID_RESET, BM_GETCHECK, 0, 0) == BST_CHECKED as isize;
    let emergency =
        SendDlgItemMessageW(hwnd, ID_EMERGENCY, BM_GETCHECK, 0, 0) == BST_CHECKED as isize;
    Ok(ConfigPatch {
        work_minutes: (values[0] != initial.work_minutes).then_some(values[0]),
        break_minutes: (values[1] != initial.break_minutes).then_some(values[1]),
        warn_seconds: (values[2] != initial.warn_seconds).then_some(values[2]),
        reset_on_unlock: (reset != initial.reset_on_unlock).then_some(reset),
        emergency_unlock: (emergency != initial.emergency_unlock).then_some(emergency),
        show_bubble: None,
        bubble_position: None,
        bubble_scale: None,
        bubble_theme: None,
    })
}

/// 用小端字段构造标准 DLGTEMPLATE，每个控件起点按 DWORD 对齐。
fn dialog_template() -> Vec<u32> {
    let mut bytes = Vec::new();
    let style = WS_POPUP
        | WS_CAPTION
        | WS_SYSMENU
        | DS_SETFONT as u32
        | DS_MODALFRAME as u32
        | DS_CENTER as u32;
    bytes.extend_from_slice(&style.to_le_bytes());
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    words(&mut bytes, &[14, 0, 0, 330, 168, 0, 0]);
    string(&mut bytes, "站会儿设置");
    words(&mut bytes, &[9]);
    string(&mut bytes, "Microsoft YaHei UI");
    for (row, label, id, range) in [
        (12, "工作时长（分钟）", ID_WORK, "1–600 分钟"),
        (39, "休息时长（分钟）", ID_BREAK, "1–120 分钟"),
        (66, "锁定前提醒（秒）", ID_WARN, "0–600 秒，0 表示不提醒"),
    ] {
        control(&mut bytes, 0x82, label, 0xffff, [12, row + 3, 98, 14], 0);
        control(
            &mut bytes,
            0x81,
            "",
            id as u16,
            [112, row, 50, 16],
            WS_TABSTOP | WS_BORDER | ES_NUMBER as u32 | ES_AUTOHSCROLL as u32,
        );
        control(&mut bytes, 0x82, range, 0xffff, [170, row + 3, 150, 14], 0);
    }
    control(
        &mut bytes,
        0x80,
        "解锁后重新计时",
        ID_RESET as u16,
        [12, 96, 145, 16],
        WS_TABSTOP | BS_AUTOCHECKBOX as u32,
    );
    control(
        &mut bytes,
        0x80,
        "紧急解锁",
        ID_EMERGENCY as u16,
        [170, 96, 145, 16],
        WS_TABSTOP | BS_AUTOCHECKBOX as u32,
    );
    control(
        &mut bytes,
        0x80,
        "开机自启动",
        ID_AUTOSTART as u16,
        [12, 116, 145, 16],
        WS_TABSTOP | BS_AUTOCHECKBOX as u32,
    );
    control(
        &mut bytes,
        0x80,
        "保存",
        IDOK as u16,
        [200, 136, 52, 20],
        WS_TABSTOP | BS_DEFPUSHBUTTON as u32,
    );
    control(
        &mut bytes,
        0x80,
        "取消",
        IDCANCEL as u16,
        [264, 136, 52, 20],
        WS_TABSTOP | BS_PUSHBUTTON as u32,
    );
    bytes.resize((bytes.len() + 3) & !3, 0);
    // Vec<u32> 保证传给 Win32 的模板本身也具有 DWORD 对齐。
    bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|chunk| u32::from_le_bytes(*chunk))
        .collect()
}

fn control(bytes: &mut Vec<u8>, class: u16, text: &str, id: u16, rect: [u16; 4], style: u32) {
    bytes.resize((bytes.len() + 3) & !3, 0);
    bytes.extend_from_slice(&(WS_CHILD | WS_VISIBLE | style).to_le_bytes());
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    words(bytes, &rect);
    words(bytes, &[id, 0xffff, class]);
    string(bytes, text);
    words(bytes, &[0]);
}

fn words(bytes: &mut Vec<u8>, values: &[u16]) {
    for value in values {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
}

fn string(bytes: &mut Vec<u8>, text: &str) {
    words(bytes, &wide(text));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn word(bytes: &[u8], offset: usize) -> u16 {
        u16::from_le_bytes(bytes[offset..offset + 2].try_into().unwrap())
    }

    fn dword(bytes: &[u8], offset: usize) -> u32 {
        u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
    }

    fn text(bytes: &[u8], cursor: &mut usize) -> String {
        let mut units = Vec::new();
        loop {
            let unit = word(bytes, *cursor);
            *cursor += 2;
            if unit == 0 {
                break;
            }
            units.push(unit);
        }
        String::from_utf16(&units).unwrap()
    }

    #[test]
    fn template_has_aligned_controls_font_and_keyboard_navigation() {
        let template = dialog_template();
        assert_eq!(template.as_ptr() as usize % 4, 0);
        let bytes: Vec<u8> = template
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect();
        let style = dword(&bytes, 0);
        assert_ne!(style & DS_SETFONT as u32, 0);
        assert_ne!(style & DS_MODALFRAME as u32, 0);
        assert_eq!(word(&bytes, 8), 14);
        assert_eq!(word(&bytes, 18), 0);
        assert_eq!(word(&bytes, 20), 0);
        let mut cursor = 22;
        assert_eq!(text(&bytes, &mut cursor), "站会儿设置");
        assert_eq!(word(&bytes, cursor), 9);
        cursor += 2;
        assert_eq!(text(&bytes, &mut cursor), "Microsoft YaHei UI");
        let mut tab_ids = Vec::new();
        let mut labels = Vec::new();
        // 独立遍历模板，验证控件数、边界、预定义类及默认按钮。
        for _ in 0..word(&bytes, 8) {
            cursor = (cursor + 3) & !3;
            let style = dword(&bytes, cursor);
            assert_ne!(style & WS_CHILD, 0);
            assert_ne!(style & WS_VISIBLE, 0);
            assert!(word(&bytes, cursor + 8) + word(&bytes, cursor + 12) <= 330);
            assert!(word(&bytes, cursor + 10) + word(&bytes, cursor + 14) <= 168);
            let id = word(&bytes, cursor + 16) as i32;
            if style & WS_TABSTOP != 0 {
                tab_ids.push(id);
            }
            assert_eq!(word(&bytes, cursor + 18), 0xffff);
            let class = word(&bytes, cursor + 20);
            assert!((0x80..=0x82).contains(&class));
            if [ID_WORK, ID_BREAK, ID_WARN].contains(&id) {
                assert_eq!(class, 0x81);
                assert_ne!(style & ES_NUMBER as u32, 0);
            }
            if [ID_RESET, ID_EMERGENCY, ID_AUTOSTART].contains(&id) {
                assert_eq!(class, 0x80);
                assert_eq!(style & 0xf, BS_AUTOCHECKBOX as u32);
            }
            if id == IDOK {
                assert_eq!(style & 0xf, BS_DEFPUSHBUTTON as u32);
            }
            cursor += 22;
            labels.push(text(&bytes, &mut cursor));
            assert_eq!(word(&bytes, cursor), 0);
            cursor += 2;
        }
        assert_eq!(
            tab_ids,
            [
                ID_WORK,
                ID_BREAK,
                ID_WARN,
                ID_RESET,
                ID_EMERGENCY,
                ID_AUTOSTART,
                IDOK,
                IDCANCEL
            ]
        );
        assert!(labels.iter().any(|label| label.contains("0 表示不提醒")));
        assert_eq!((cursor + 3) & !3, bytes.len());
    }
}
