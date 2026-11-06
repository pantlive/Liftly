//! 配置解析。这里只包含平台无关逻辑，方便在非 Windows 平台上跑单元测试。

/// 运行时配置。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub work_minutes: u32,
    pub break_minutes: u32,
    pub warn_seconds: u32,
    pub reset_on_unlock: bool,
    pub emergency_unlock: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            work_minutes: 45,
            break_minutes: 5,
            warn_seconds: 60,
            reset_on_unlock: true,
            emergency_unlock: true,
        }
    }
}

impl Config {
    /// 在默认值基础上解析 INI 文本，忽略注释、未知键和非法值。
    pub fn parse_ini(text: &str) -> Self {
        let mut cfg = Self::default();
        for raw_line in text.lines() {
            let line = raw_line.trim_start_matches('\u{feff}').trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let key = key.trim().to_ascii_lowercase();
            let value = value.trim();
            // 允许行尾注释。
            let value = match value.find(['#', ';']) {
                Some(pos) => value[..pos].trim(),
                None => value,
            };
            cfg.apply_value(&key, value);
        }
        cfg.normalize();
        cfg
    }

    /// 应用命令行覆盖项，支持 `--work 45` 和 `--work=45` 两种写法。
    pub fn apply_args(&mut self, mut args: impl Iterator<Item = String>) {
        while let Some(arg) = args.next() {
            let (key, inline_value) = match arg.split_once('=') {
                Some((key, value)) => (key, Some(value)),
                None => (arg.as_str(), None),
            };
            let key = match key {
                "--work" => "work_minutes",
                "--break" => "break_minutes",
                "--warn" => "warn_seconds",
                _ => continue,
            };
            let value = match inline_value {
                Some(value) => value.to_owned(),
                None => match args.next() {
                    Some(value) => value,
                    None => break,
                },
            };
            self.apply_value(key, value.trim());
        }
        self.normalize();
    }

    fn apply_value(&mut self, key: &str, value: &str) {
        match key {
            "work_minutes" => {
                if let Ok(v) = value.parse::<u32>() {
                    if (1..=600).contains(&v) {
                        self.work_minutes = v;
                    }
                }
            }
            "break_minutes" => {
                if let Ok(v) = value.parse::<u32>() {
                    if (1..=120).contains(&v) {
                        self.break_minutes = v;
                    }
                }
            }
            "warn_seconds" => {
                if let Ok(v) = value.parse::<u32>() {
                    if v <= 600 {
                        self.warn_seconds = v;
                    }
                }
            }
            "reset_on_unlock" => {
                if let Some(v) = parse_bool(value) {
                    self.reset_on_unlock = v;
                }
            }
            "emergency_unlock" => {
                if let Some(v) = parse_bool(value) {
                    self.emergency_unlock = v;
                }
            }
            _ => {}
        }
    }

    /// 提醒时间不能达到或超过工作时长。
    fn normalize(&mut self) {
        let work_seconds = self.work_minutes.saturating_mul(60);
        if self.warn_seconds >= work_seconds {
            self.warn_seconds = work_seconds.saturating_sub(1);
        }
    }
}

fn parse_bool(value: &str) -> Option<bool> {
    if value.eq_ignore_ascii_case("true")
        || value.eq_ignore_ascii_case("yes")
        || value.eq_ignore_ascii_case("on")
        || value == "1"
    {
        Some(true)
    } else if value.eq_ignore_ascii_case("false")
        || value.eq_ignore_ascii_case("no")
        || value.eq_ignore_ascii_case("off")
        || value == "0"
    {
        Some(false)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_expected() {
        let cfg = Config::default();
        assert_eq!(cfg.work_minutes, 45);
        assert_eq!(cfg.break_minutes, 5);
        assert_eq!(cfg.warn_seconds, 60);
        assert!(cfg.reset_on_unlock);
        assert!(cfg.emergency_unlock);
    }

    #[test]
    fn parses_values_comments_and_blank_lines() {
        let cfg = Config::parse_ini(
            "\u{feff}# 注释\n\
             ; 另一种注释\n\
             work_minutes = 30\n\
             break_minutes=10\n\
             warn_seconds = 15 ; 行尾注释\n\
             reset_on_unlock = false\n\
             emergency_unlock = no\n",
        );
        assert_eq!(cfg.work_minutes, 30);
        assert_eq!(cfg.break_minutes, 10);
        assert_eq!(cfg.warn_seconds, 15);
        assert!(!cfg.reset_on_unlock);
        assert!(!cfg.emergency_unlock);
    }

    #[test]
    fn ignores_unknown_keys_and_invalid_values() {
        let cfg = Config::parse_ini(
            "unknown_key = 123\n\
             work_minutes = 0\n\
             break_minutes = 121\n\
             warn_seconds = abc\n\
             reset_on_unlock = maybe\n\
             emergency_unlock = 2\n",
        );
        assert_eq!(cfg, Config::default());
    }

    #[test]
    fn accepts_boolean_aliases() {
        let cfg = Config::parse_ini(
            "reset_on_unlock = 0\n\
             emergency_unlock = YES\n",
        );
        assert!(!cfg.reset_on_unlock);
        assert!(cfg.emergency_unlock);
    }

    #[test]
    fn cli_overrides_ini() {
        let mut cfg =
            Config::parse_ini("work_minutes = 20\nbreak_minutes = 2\nwarn_seconds = 10\n");
        cfg.apply_args(
            ["--work", "50", "--break=7", "--warn", "30"]
                .into_iter()
                .map(str::to_owned),
        );
        assert_eq!(cfg.work_minutes, 50);
        assert_eq!(cfg.break_minutes, 7);
        assert_eq!(cfg.warn_seconds, 30);
    }

    #[test]
    fn cli_ignores_invalid_and_out_of_range_values() {
        let mut cfg = Config::default();
        cfg.apply_args(
            ["--work", "0", "--break", "999", "--warn", "700", "junk"]
                .into_iter()
                .map(str::to_owned),
        );
        assert_eq!(cfg, Config::default());
    }

    #[test]
    fn warn_is_truncated_to_work_time() {
        let cfg = Config::parse_ini("work_minutes = 1\nwarn_seconds = 60\n");
        assert_eq!(cfg.warn_seconds, 59);

        let cfg = Config::parse_ini("work_minutes = 1\nwarn_seconds = 600\n");
        assert_eq!(cfg.warn_seconds, 59);

        let cfg = Config::parse_ini("work_minutes = 2\nwarn_seconds = 60\n");
        assert_eq!(cfg.warn_seconds, 60);
    }

    #[test]
    fn cli_warn_is_truncated_to_work_time() {
        let mut cfg = Config::default();
        cfg.apply_args(
            ["--work", "1", "--warn", "60"]
                .into_iter()
                .map(str::to_owned),
        );
        assert_eq!(cfg.warn_seconds, 59);
    }

    #[test]
    fn missing_cli_value_does_not_panic() {
        let mut cfg = Config::default();
        cfg.apply_args(["--work"].into_iter().map(str::to_owned));
        assert_eq!(cfg, Config::default());
    }
}
