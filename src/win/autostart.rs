//! 通过 HKCU Run 注册表项管理开机自启。

use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_MORE_DATA, ERROR_SUCCESS};
use windows_sys::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegGetValueW, RegOpenKeyExW, RegSetValueExW,
    HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ, RRF_RT_REG_SZ,
};

use super::wide;
use crate::autostart::{command_targets, run_command, VALUE_NAME};
use crate::i18n;

const RUN_SUBKEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

struct RegKey(HKEY);

impl Drop for RegKey {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                let _ = RegCloseKey(self.0);
            }
            self.0 = null_mut();
        }
    }
}

/// 当前 exe 是否已写入 HKCU Run，且命令行指向本程序。
pub fn is_enabled() -> bool {
    let Some(exe) = current_exe_string() else {
        return false;
    };
    match read_run_command() {
        Some(value) => command_targets(&value, &exe),
        None => false,
    }
}

/// 开启时写入带 `--autostart` 的命令行；关闭时删除该值（值不存在也算成功）。
pub fn set_enabled(on: bool) -> Result<(), String> {
    if on {
        write_run_command()
    } else {
        delete_run_command()
    }
}

fn current_exe_string() -> Option<String> {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.into_os_string().into_string().ok())
}

fn read_run_command() -> Option<String> {
    let subkey = wide(RUN_SUBKEY);
    let name = wide(VALUE_NAME);
    let mut buf = vec![0_u16; 1024];
    loop {
        let mut size = (buf.len() * 2) as u32;
        let status = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                subkey.as_ptr(),
                name.as_ptr(),
                RRF_RT_REG_SZ,
                null_mut(),
                buf.as_mut_ptr().cast(),
                &mut size,
            )
        };
        if status == ERROR_SUCCESS {
            let units = (size as usize / 2).min(buf.len());
            let text = &buf[..units];
            let end = text
                .iter()
                .position(|&unit| unit == 0)
                .unwrap_or(text.len());
            return Some(String::from_utf16_lossy(&text[..end]));
        }
        if status != ERROR_MORE_DATA {
            return None;
        }
        let needed = (size as usize).div_ceil(2).max(buf.len().saturating_add(1));
        if needed > 32 * 1024 {
            return None;
        }
        buf.resize(needed, 0);
    }
}

fn write_run_command() -> Result<(), String> {
    let lang = i18n::current();
    let exe = current_exe_string().ok_or_else(|| i18n::autostart_path_error(lang).to_owned())?;
    let command = wide(&run_command(&exe));
    let name = wide(VALUE_NAME);
    let key = create_run_key()?;
    let bytes = (command.len() * 2) as u32;
    let status = unsafe {
        RegSetValueExW(
            key.0,
            name.as_ptr(),
            0,
            REG_SZ,
            command.as_ptr().cast(),
            bytes,
        )
    };
    if status == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(i18n::autostart_write_error(lang, status))
    }
}

fn delete_run_command() -> Result<(), String> {
    let lang = i18n::current();
    let key = match open_run_key() {
        Ok(key) => key,
        Err(ERROR_FILE_NOT_FOUND) => return Ok(()),
        Err(status) => return Err(i18n::autostart_open_key_error(lang, status)),
    };
    let name = wide(VALUE_NAME);
    let status = unsafe { RegDeleteValueW(key.0, name.as_ptr()) };
    if status == ERROR_SUCCESS || status == ERROR_FILE_NOT_FOUND {
        Ok(())
    } else {
        Err(i18n::autostart_delete_error(lang, status))
    }
}

fn create_run_key() -> Result<RegKey, String> {
    let subkey = wide(RUN_SUBKEY);
    let mut handle = null_mut();
    let status = unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            subkey.as_ptr(),
            0,
            null(),
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            null(),
            &mut handle,
            null_mut(),
        )
    };
    if status == ERROR_SUCCESS && !handle.is_null() {
        Ok(RegKey(handle))
    } else {
        Err(i18n::autostart_create_key_error(i18n::current(), status))
    }
}

fn open_run_key() -> Result<RegKey, u32> {
    let subkey = wide(RUN_SUBKEY);
    let mut handle = null_mut();
    let status = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            subkey.as_ptr(),
            0,
            KEY_SET_VALUE,
            &mut handle,
        )
    };
    if status == ERROR_SUCCESS && !handle.is_null() {
        Ok(RegKey(handle))
    } else {
        Err(status)
    }
}
