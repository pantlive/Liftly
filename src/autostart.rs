//! 开机自启命令行的平台无关解析：注册表值和当前 exe 的对应关系。

/// 写入 HKCU Run 的值名称。
pub const VALUE_NAME: &str = "Liftly";
/// 开机启动时附带的参数，第二个实例据此静默退出。
pub const AUTOSTART_ARG: &str = "--autostart";

/// 生成写入注册表的命令行：带双引号的 exe 路径，后接 `--autostart`。
pub fn run_command(exe: &str) -> String {
    format!("\"{exe}\" {AUTOSTART_ARG}")
}

/// 注册表值是否指向当前 exe。支持带引号和不带引号两种写法，忽略后面的参数。
pub fn command_targets(value: &str, exe: &str) -> bool {
    match extract_exe(value) {
        Some(path) => path.eq_ignore_ascii_case(exe),
        None => false,
    }
}

/// 命令行是否包含开机自启参数。
pub fn is_autostart_launch(mut args: impl Iterator<Item = String>) -> bool {
    args.any(|arg| arg == AUTOSTART_ARG)
}

/// 从注册表命令行取出 exe 路径；空值或不完整引号返回 `None`。
fn extract_exe(value: &str) -> Option<&str> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    if let Some(rest) = value.strip_prefix('"') {
        let end = rest.find('"')?;
        let path = &rest[..end];
        if path.is_empty() {
            None
        } else {
            Some(path)
        }
    } else {
        value
            .split([' ', '\t'])
            .next()
            .filter(|path| !path.is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoted_path_with_spaces_round_trips() {
        assert_eq!(VALUE_NAME, "Liftly");
        let exe = r"C:\Program Files\Liftly\liftly.exe";
        let command = run_command(exe);
        assert_eq!(command, r#""C:\Program Files\Liftly\liftly.exe" --autostart"#);
        assert!(command_targets(&command, exe));
        assert!(command_targets(&command, r"c:\program files\liftly\LIFTLY.EXE"));
    }

    #[test]
    fn unquoted_path_matches_and_ignores_args() {
        let exe = r"C:\Liftly\liftly.exe";
        assert_eq!(run_command(exe), r#""C:\Liftly\liftly.exe" --autostart"#);
        assert!(command_targets(r"C:\Liftly\liftly.exe --autostart", exe));
        assert!(command_targets(r"C:\Liftly\liftly.exe", exe));
        assert!(command_targets(
            r#""C:\Liftly\liftly.exe" --work 45 --autostart"#,
            exe
        ));
    }

    #[test]
    fn other_path_does_not_match() {
        assert!(!command_targets(
            r#""C:\Other\liftly.exe" --autostart"#,
            r"C:\Liftly\liftly.exe"
        ));
        assert!(!command_targets(
            r"D:\Liftly\liftly.exe --autostart",
            r"C:\Liftly\liftly.exe"
        ));
    }

    #[test]
    fn empty_and_incomplete_quotes_do_not_match() {
        let exe = r"C:\Liftly\liftly.exe";
        assert!(!command_targets("", exe));
        assert!(!command_targets("   ", exe));
        assert!(!command_targets(r#""C:\Liftly\liftly.exe"#, exe));
        assert!(!command_targets("\"\"", exe));
        assert!(!command_targets("\"", exe));
    }

    #[test]
    fn detects_autostart_argument() {
        assert!(is_autostart_launch(["--autostart".to_owned()].into_iter()));
        assert!(is_autostart_launch(
            ["--work".to_owned(), "45".to_owned(), "--autostart".to_owned()].into_iter()
        ));
        assert!(!is_autostart_launch(
            ["--work".to_owned(), "45".to_owned()].into_iter()
        ));
        assert!(!is_autostart_launch(std::iter::empty()));
        assert!(!is_autostart_launch(
            ["--autostart=1".to_owned()].into_iter()
        ));
    }
}
