//! 配置解析。这里只包含平台无关逻辑，方便在非 Windows 平台上跑单元测试。

use std::ops::RangeInclusive;

use crate::i18n::LanguagePref;

/// 工作时长的合法范围（分钟）。
pub const WORK_RANGE: RangeInclusive<u32> = 1..=600;
/// 休息时长的合法范围（分钟）。
pub const BREAK_RANGE: RangeInclusive<u32> = 1..=120;
/// 提醒秒数的上限。
pub const WARN_MAX: u32 = 600;

/// 托盘菜单的常用档位。
pub const WORK_PRESETS: [u32; 5] = [25, 30, 45, 60, 90];
pub const BREAK_PRESETS: [u32; 4] = [3, 5, 10, 15];
pub const WARN_PRESETS: [u32; 4] = [0, 30, 60, 120];
/// 气泡大小百分比的合法范围。
pub const BUBBLE_SCALE_RANGE: RangeInclusive<u32> = 50..=300;
/// 托盘菜单的气泡大小预设档位。
pub const BUBBLE_SCALE_PRESETS: [u32; 5] = [80, 100, 125, 150, 200];
/// 托盘菜单的气泡皮肤预设。
pub const BUBBLE_THEME_PRESETS: [BubbleTheme; 4] = [
    BubbleTheme::Green,
    BubbleTheme::Blue,
    BubbleTheme::Orange,
    BubbleTheme::Dark,
];

/// 倒计时气泡的保存位置；`Auto` 使用工作区右上角。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum BubblePosition {
    #[default]
    Auto,
    At(i32, i32),
}

impl BubblePosition {
    /// 返回供 Windows 定位使用的坐标；自动位置返回 `None`。
    pub const fn coordinates(self) -> Option<(i32, i32)> {
        match self {
            Self::Auto => None,
            Self::At(x, y) => Some((x, y)),
        }
    }

    fn parse(value: &str) -> Option<Self> {
        if value.eq_ignore_ascii_case("auto") {
            return Some(Self::Auto);
        }
        let (x, y) = value.split_once(',')?;
        let x = x.trim().parse::<i32>().ok()?;
        let y = y.trim().parse::<i32>().ok()?;
        Some(Self::At(x, y))
    }
}

/// 倒计时气泡皮肤；INI 中写作 `green` / `blue` / `orange` / `dark`。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum BubbleTheme {
    #[default]
    Green,
    Blue,
    Orange,
    Dark,
}

impl BubbleTheme {
    pub fn parse(value: &str) -> Option<Self> {
        if value.eq_ignore_ascii_case("green") {
            Some(Self::Green)
        } else if value.eq_ignore_ascii_case("blue") {
            Some(Self::Blue)
        } else if value.eq_ignore_ascii_case("orange") {
            Some(Self::Orange)
        } else if value.eq_ignore_ascii_case("dark") {
            Some(Self::Dark)
        } else {
            None
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Green => "green",
            Self::Blue => "blue",
            Self::Orange => "orange",
            Self::Dark => "dark",
        }
    }
}

/// 只包含本次用户修改的字段，避免把命令行覆盖值写回磁盘。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConfigPatch {
    pub work_minutes: Option<u32>,
    pub break_minutes: Option<u32>,
    pub warn_seconds: Option<u32>,
    pub reset_on_unlock: Option<bool>,
    pub emergency_unlock: Option<bool>,
    pub show_bubble: Option<bool>,
    pub bubble_position: Option<BubblePosition>,
    pub bubble_scale: Option<u32>,
    pub bubble_theme: Option<BubbleTheme>,
}

impl ConfigPatch {
    fn changes(&self, key: &str) -> bool {
        match key {
            "work_minutes" => self.work_minutes.is_some(),
            "break_minutes" => self.break_minutes.is_some(),
            "warn_seconds" => self.warn_seconds.is_some(),
            "reset_on_unlock" => self.reset_on_unlock.is_some(),
            "emergency_unlock" => self.emergency_unlock.is_some(),
            "show_bubble" => self.show_bubble.is_some(),
            "bubble_position" => self.bubble_position.is_some(),
            "bubble_scale" => self.bubble_scale.is_some(),
            "bubble_theme" => self.bubble_theme.is_some(),
            _ => false,
        }
    }
}

/// 以磁盘配置为基准合并修改，保留原文件的注释及换行格式。
pub fn merge_into_ini(original: &str, patch: &ConfigPatch) -> Result<String, String> {
    let cfg = Config::parse_ini(original).apply_patch(patch)?;
    Ok(if original.trim_start_matches('\u{feff}').is_empty() {
        cfg.to_ini_text(original)
    } else {
        cfg.render_ini(original, Some(patch))
    })
}

/// 写回 INI 时使用的已知键及其注释，顺序与配置文件一致。
const INI_KEYS: [&str; 9] = [
    "work_minutes",
    "break_minutes",
    "warn_seconds",
    "reset_on_unlock",
    "emergency_unlock",
    "show_bubble",
    "bubble_position",
    "bubble_scale",
    "bubble_theme",
];

const INI_COMMENTS: [&str; 9] = [
    "# 工作时长（分钟），1-600",
    "# 休息时长（分钟），1-120",
    "# 锁定前提醒秒数，0-600；0 表示不提醒",
    "# 锁屏期间暂停计时，解锁或睡眠唤醒后是否重新开始一个完整工作周期",
    "# 是否允许同时按住 Ctrl、Alt、Shift（左右均可）再按 F12 紧急解锁",
    "# 是否显示桌面倒计时气泡",
    "# 气泡位置：auto（工作区右上角）或 x,y（例如 1200,80），拖动后自动保存",
    "# 气泡大小百分比，50-300；100 为标准大小",
    "# 气泡皮肤：green/blue/orange/dark",
];

/// 运行时配置。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub work_minutes: u32,
    pub break_minutes: u32,
    pub warn_seconds: u32,
    pub reset_on_unlock: bool,
    pub emergency_unlock: bool,
    pub show_bubble: bool,
    pub bubble_position: BubblePosition,
    pub bubble_scale: u32,
    pub bubble_theme: BubbleTheme,
    pub language: LanguagePref,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            work_minutes: 45,
            break_minutes: 5,
            warn_seconds: 60,
            reset_on_unlock: true,
            emergency_unlock: true,
            show_bubble: true,
            bubble_position: BubblePosition::Auto,
            bubble_scale: 100,
            bubble_theme: BubbleTheme::Green,
            language: LanguagePref::Auto,
        }
    }
}

impl Config {
    /// 校验并应用部分修改；未修改的字段沿用当前配置。
    pub fn apply_patch(&self, patch: &ConfigPatch) -> Result<Self, String> {
        let mut cfg = self.with_times(
            patch.work_minutes.unwrap_or(self.work_minutes),
            patch.break_minutes.unwrap_or(self.break_minutes),
            patch.warn_seconds.unwrap_or(self.warn_seconds),
        )?;
        cfg.reset_on_unlock = patch.reset_on_unlock.unwrap_or(self.reset_on_unlock);
        cfg.emergency_unlock = patch.emergency_unlock.unwrap_or(self.emergency_unlock);
        cfg.show_bubble = patch.show_bubble.unwrap_or(self.show_bubble);
        cfg.bubble_position = patch.bubble_position.unwrap_or(self.bubble_position);
        if let Some(scale) = patch.bubble_scale {
            if !BUBBLE_SCALE_RANGE.contains(&scale) {
                return Err(format!(
                    "气泡大小必须在 {}% 到 {}% 之间",
                    BUBBLE_SCALE_RANGE.start(),
                    BUBBLE_SCALE_RANGE.end()
                ));
            }
            cfg.bubble_scale = scale;
        }
        cfg.bubble_theme = patch.bubble_theme.unwrap_or(self.bubble_theme);
        Ok(cfg)
    }

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
                    if WORK_RANGE.contains(&v) {
                        self.work_minutes = v;
                    }
                }
            }
            "break_minutes" => {
                if let Ok(v) = value.parse::<u32>() {
                    if BREAK_RANGE.contains(&v) {
                        self.break_minutes = v;
                    }
                }
            }
            "warn_seconds" => {
                if let Ok(v) = value.parse::<u32>() {
                    if v <= WARN_MAX {
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
            "show_bubble" => {
                if let Some(v) = parse_bool(value) {
                    self.show_bubble = v;
                }
            }
            "bubble_position" => {
                if let Some(v) = BubblePosition::parse(value) {
                    self.bubble_position = v;
                }
            }
            "bubble_scale" => {
                if let Ok(v) = value.parse::<u32>() {
                    if BUBBLE_SCALE_RANGE.contains(&v) {
                        self.bubble_scale = v;
                    }
                }
            }
            "bubble_theme" => {
                if let Some(v) = BubbleTheme::parse(value) {
                    self.bubble_theme = v;
                }
            }
            "language" => {
                if let Some(v) = LanguagePref::parse(value) {
                    self.language = v;
                }
            }
            _ => {}
        }
    }

    /// 用三个时长值生成新配置，任一项越界时返回中文错误。
    pub fn with_times(
        &self,
        work_minutes: u32,
        break_minutes: u32,
        warn_seconds: u32,
    ) -> Result<Config, String> {
        if !WORK_RANGE.contains(&work_minutes) {
            return Err(format!(
                "工作时长必须在 {} 到 {} 分钟之间",
                WORK_RANGE.start(),
                WORK_RANGE.end()
            ));
        }
        if !BREAK_RANGE.contains(&break_minutes) {
            return Err(format!(
                "休息时长必须在 {} 到 {} 分钟之间",
                BREAK_RANGE.start(),
                BREAK_RANGE.end()
            ));
        }
        if warn_seconds > WARN_MAX {
            return Err(format!("提醒秒数必须在 0 到 {} 秒之间", WARN_MAX));
        }

        let mut cfg = self.clone();
        cfg.work_minutes = work_minutes;
        cfg.break_minutes = break_minutes;
        cfg.warn_seconds = warn_seconds;
        cfg.normalize();
        Ok(cfg)
    }

    /// 在原 INI 文本基础上写回全部已知键，保留注释、空行、未知键和原有顺序。
    pub fn to_ini_text(&self, original: &str) -> String {
        self.render_ini(original, None)
    }

    fn render_ini(&self, original: &str, patch: Option<&ConfigPatch>) -> String {
        let (bom, text) = match original.strip_prefix('\u{feff}') {
            Some(rest) => ("\u{feff}", rest),
            None => ("", original),
        };
        let newline = if text.contains("\r\n") { "\r\n" } else { "\n" };
        let ends_with_newline = text.ends_with('\n');

        // 空文件直接生成一份带注释的完整配置。
        if text.is_empty() {
            let mut lines = vec![
                "# 站会儿配置文件".to_owned(),
                "# 复制为 liftly.ini，放在 liftly.exe 同目录。".to_owned(),
                "# 以 # 或 ; 开头的行是注释。".to_owned(),
            ];
            for (index, key) in INI_KEYS.iter().enumerate() {
                lines.push(String::new());
                lines.push(INI_COMMENTS[index].to_owned());
                lines.push(format!("{} = {}", key, self.value_of(key)));
            }
            let mut output = String::from(bom);
            output.push_str(&lines.join(newline));
            output.push_str(newline);
            return output;
        }

        let mut lines: Vec<String> = text
            .split('\n')
            .map(|line| line.strip_suffix('\r').unwrap_or(line).to_owned())
            .collect();
        if ends_with_newline {
            lines.pop();
        }

        let mut present = [false; INI_KEYS.len()];
        for line in &mut lines {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with(';') {
                continue;
            }
            let Some((key, value)) = trimmed.split_once('=') else {
                continue;
            };
            let key = key.trim().to_ascii_lowercase();
            let Some(index) = INI_KEYS.iter().position(|known| *known == key.as_str()) else {
                continue;
            };
            present[index] = true;
            // 局部保存时连未修改键的格式和原始值也保留，避免归一化改动提醒秒数。
            if patch.is_some_and(|patch| !patch.changes(&key)) {
                continue;
            }
            // 行尾注释原样保留，值与注释之间统一用一个空格分隔。
            let comment = value
                .find(['#', ';'])
                .map(|pos| value[pos..].trim_end().to_owned());
            let rendered = format!("{} = {}", INI_KEYS[index], self.value_of(INI_KEYS[index]));
            *line = match comment {
                Some(comment) => format!("{} {}", rendered, comment),
                None => rendered,
            };
        }

        let mut appended = false;
        for (index, key) in INI_KEYS.iter().enumerate() {
            if present[index] {
                continue;
            }
            appended = true;
            // 段与段之间空一行，便于阅读。
            if matches!(lines.last(), Some(line) if !line.is_empty()) {
                lines.push(String::new());
            }
            lines.push(INI_COMMENTS[index].to_owned());
            lines.push(format!("{} = {}", key, self.value_of(key)));
        }

        let mut output = String::from(bom);
        output.push_str(&lines.join(newline));
        if appended || ends_with_newline {
            output.push_str(newline);
        }
        output
    }

    fn value_of(&self, key: &str) -> String {
        match key {
            "work_minutes" => self.work_minutes.to_string(),
            "break_minutes" => self.break_minutes.to_string(),
            "warn_seconds" => self.warn_seconds.to_string(),
            "reset_on_unlock" => self.reset_on_unlock.to_string(),
            "emergency_unlock" => self.emergency_unlock.to_string(),
            "show_bubble" => self.show_bubble.to_string(),
            "bubble_position" => match self.bubble_position {
                BubblePosition::Auto => "auto".to_owned(),
                BubblePosition::At(x, y) => format!("{x},{y}"),
            },
            "bubble_scale" => self.bubble_scale.to_string(),
            "bubble_theme" => self.bubble_theme.as_str().to_owned(),
            "language" => self.language.as_str().to_owned(),
            _ => String::new(),
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
    fn presets_are_valid_and_include_defaults() {
        let cfg = Config::default();
        assert!(WORK_PRESETS.iter().all(|v| WORK_RANGE.contains(v)));
        assert!(BREAK_PRESETS.iter().all(|v| BREAK_RANGE.contains(v)));
        assert!(WARN_PRESETS.iter().all(|v| *v <= WARN_MAX));
        assert!(BUBBLE_SCALE_PRESETS
            .iter()
            .all(|v| BUBBLE_SCALE_RANGE.contains(v)));
        assert_eq!(
            BUBBLE_THEME_PRESETS,
            [
                BubbleTheme::Green,
                BubbleTheme::Blue,
                BubbleTheme::Orange,
                BubbleTheme::Dark
            ]
        );
        for theme in BUBBLE_THEME_PRESETS {
            assert_eq!(BubbleTheme::parse(theme.as_str()), Some(theme));
        }
        assert!(WORK_PRESETS.contains(&cfg.work_minutes));
        assert!(BREAK_PRESETS.contains(&cfg.break_minutes));
        assert!(WARN_PRESETS.contains(&cfg.warn_seconds));
        assert!(BUBBLE_SCALE_PRESETS.contains(&cfg.bubble_scale));
        assert!(BUBBLE_THEME_PRESETS.contains(&cfg.bubble_theme));
    }

    #[test]
    fn merge_uses_disk_values_instead_of_command_line_overrides() {
        let original = "work_minutes = 37\nbreak_minutes = 5\nwarn_seconds = 120\nreset_on_unlock = false\nemergency_unlock = true\nshow_bubble = true\nbubble_position = auto\n";
        let mut runtime = Config::parse_ini(original);
        runtime.apply_args(["--work".to_owned(), "10".to_owned()].into_iter());
        let patch = ConfigPatch {
            break_minutes: Some(10),
            emergency_unlock: Some(false),
            show_bubble: Some(false),
            ..ConfigPatch::default()
        };
        let applied = runtime.apply_patch(&patch).unwrap();
        let saved = Config::parse_ini(&merge_into_ini(original, &patch).unwrap());
        assert_eq!(applied.work_minutes, 10);
        assert_eq!(saved.work_minutes, 37);
        assert_eq!(saved.break_minutes, 10);
        assert_eq!(saved.warn_seconds, 120);
        assert!(!saved.reset_on_unlock);
        assert!(!saved.emergency_unlock);
        assert!(!saved.show_bubble);
    }

    #[test]
    fn merge_keeps_untouched_raw_values_and_formatting() {
        let original = "WORK_MINUTES=1\r\nbreak_minutes = 5\r\nwarn_seconds=120 # 启动时截断\r\nreset_on_unlock=no\r\nemergency_unlock=on\r\nshow_bubble=yes\r\nbubble_position=auto\r\nbubble_scale=100\r\nbubble_theme=green\r\n";
        let patch = ConfigPatch {
            break_minutes: Some(10),
            ..ConfigPatch::default()
        };
        assert_eq!(
            merge_into_ini(original, &patch).unwrap(),
            original.replace("break_minutes = 5", "break_minutes = 10")
        );
    }

    #[test]
    fn merge_preserves_bom_crlf_and_comments_and_creates_missing_file() {
        let patch = ConfigPatch {
            work_minutes: Some(30),
            ..ConfigPatch::default()
        };
        let original = "\u{feff}# 自定配置\r\nwork_minutes = 37 ; 保留\r\nunknown = yes\r\n";
        let text = merge_into_ini(original, &patch).unwrap();
        assert!(
            text.starts_with("\u{feff}# 自定配置\r\nwork_minutes = 30 ; 保留\r\nunknown = yes\r\n")
        );
        assert!(!text.replace("\r\n", "").contains('\n'));
        let created = merge_into_ini("", &patch).unwrap();
        assert!(created.contains("# 工作时长"));
        assert_eq!(Config::parse_ini(&created).work_minutes, 30);
    }

    #[test]
    fn patches_reject_out_of_range_values_with_chinese_errors() {
        for (patch, message) in [
            (
                ConfigPatch {
                    work_minutes: Some(0),
                    ..ConfigPatch::default()
                },
                "工作时长",
            ),
            (
                ConfigPatch {
                    break_minutes: Some(121),
                    ..ConfigPatch::default()
                },
                "休息时长",
            ),
            (
                ConfigPatch {
                    warn_seconds: Some(601),
                    ..ConfigPatch::default()
                },
                "提醒秒数",
            ),
            (
                ConfigPatch {
                    bubble_scale: Some(49),
                    ..ConfigPatch::default()
                },
                "气泡大小",
            ),
            (
                ConfigPatch {
                    bubble_scale: Some(301),
                    ..ConfigPatch::default()
                },
                "气泡大小",
            ),
        ] {
            assert!(merge_into_ini("", &patch).unwrap_err().contains(message));
        }
    }

    #[test]
    fn defaults_are_expected() {
        let cfg = Config::default();
        assert_eq!(cfg.work_minutes, 45);
        assert_eq!(cfg.break_minutes, 5);
        assert_eq!(cfg.warn_seconds, 60);
        assert!(cfg.reset_on_unlock);
        assert!(cfg.emergency_unlock);
        assert!(cfg.show_bubble);
        assert_eq!(cfg.bubble_position, BubblePosition::Auto);
        assert_eq!(cfg.bubble_scale, 100);
        assert_eq!(cfg.bubble_theme, BubbleTheme::Green);
        assert_eq!(cfg.language, LanguagePref::Auto);
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
             emergency_unlock = no\n\
             show_bubble = off\n\
             bubble_position = 1200,80\n\
             bubble_scale = 150\n\
             bubble_theme = blue\n",
        );
        assert_eq!(cfg.work_minutes, 30);
        assert_eq!(cfg.break_minutes, 10);
        assert_eq!(cfg.warn_seconds, 15);
        assert!(!cfg.reset_on_unlock);
        assert!(!cfg.emergency_unlock);
        assert!(!cfg.show_bubble);
        assert_eq!(cfg.bubble_position, BubblePosition::At(1200, 80));
        assert_eq!(cfg.bubble_scale, 150);
        assert_eq!(cfg.bubble_theme, BubbleTheme::Blue);
    }

    #[test]
    fn ignores_unknown_keys_and_invalid_values() {
        let cfg = Config::parse_ini(
            "unknown_key = 123\n\
             work_minutes = 0\n\
             break_minutes = 121\n\
             warn_seconds = abc\n\
             reset_on_unlock = maybe\n\
             emergency_unlock = 2\n\
             show_bubble = sometimes\n\
             bubble_position = 12,abc\n\
             bubble_scale = abc\n\
             bubble_scale = 999\n\
             bubble_theme = lime\n\
             bubble_theme = 1\n",
        );
        assert_eq!(cfg, Config::default());
    }

    #[test]
    fn accepts_boolean_aliases() {
        let cfg = Config::parse_ini(
            "reset_on_unlock = 0\n\
             emergency_unlock = YES\n\
             show_bubble = 0\n",
        );
        assert!(!cfg.reset_on_unlock);
        assert!(cfg.emergency_unlock);
        assert!(!cfg.show_bubble);
    }

    #[test]
    fn parses_bubble_position_and_ignores_invalid_values() {
        assert_eq!(Config::default().bubble_position, BubblePosition::Auto);
        assert_eq!(BubblePosition::Auto.coordinates(), None);
        assert_eq!(BubblePosition::At(-12, 34).coordinates(), Some((-12, 34)));
        for (value, expected) in [
            ("auto", BubblePosition::Auto),
            ("AUTO", BubblePosition::Auto),
            ("1200,80", BubblePosition::At(1200, 80)),
            ("  -1920, 100 ; 副屏 ", BubblePosition::At(-1920, 100)),
        ] {
            let cfg = Config::parse_ini(&format!("bubble_position = {value}\n"));
            assert_eq!(cfg.bubble_position, expected, "value={value}");
        }
        for value in ["", "auto,1", "1,2,3", "12,abc", "2147483648,0"] {
            let cfg = Config::parse_ini(&format!("bubble_position = {value}\n"));
            assert_eq!(cfg.bubble_position, BubblePosition::Auto, "value={value}");
        }
    }

    #[test]
    fn saving_bubble_position_only_touches_that_key() {
        let original = "work_minutes=37\nbreak_minutes = 5\nwarn_seconds=120\nreset_on_unlock=no\nemergency_unlock=on\nshow_bubble=yes\nbubble_position=auto\nbubble_scale=100\nbubble_theme=green\n";
        let runtime = Config::parse_ini(original);
        let patch = ConfigPatch {
            bubble_position: Some(BubblePosition::At(1200, 80)),
            ..ConfigPatch::default()
        };
        let applied = runtime.apply_patch(&patch).unwrap();
        assert_eq!(applied.bubble_position, BubblePosition::At(1200, 80));
        assert_eq!(applied.work_minutes, 37);

        let saved_text = merge_into_ini(original, &patch).unwrap();
        assert_eq!(
            saved_text,
            original.replace("bubble_position=auto", "bubble_position = 1200,80")
        );
        assert_eq!(Config::parse_ini(&saved_text), applied);
    }

    #[test]
    fn saving_other_settings_preserves_saved_bubble_position() {
        let original = "work_minutes = 37\nbreak_minutes = 5\nwarn_seconds = 120\nreset_on_unlock = false\nemergency_unlock = true\nshow_bubble = true\nbubble_position = 1200,80\n";
        let runtime = Config::parse_ini(original);
        let patch = ConfigPatch {
            work_minutes: Some(30),
            ..ConfigPatch::default()
        };
        let applied = runtime.apply_patch(&patch).unwrap();
        assert_eq!(applied.bubble_position, BubblePosition::At(1200, 80));

        let saved_text = merge_into_ini(original, &patch).unwrap();
        assert!(saved_text.contains("work_minutes = 30\n"));
        assert!(saved_text.contains("bubble_position = 1200,80\n"));
        assert_eq!(Config::parse_ini(&saved_text), applied);
    }

    #[test]
    fn saving_position_upgrades_old_ini_without_rewriting_existing_settings() {
        let original = "\u{feff}# 旧版配置\r\nWORK_MINUTES=37\r\nbreak_minutes=5\r\nwarn_seconds=120\r\nreset_on_unlock=no\r\nemergency_unlock=on\r\nshow_bubble=yes\r\n";
        let patch = ConfigPatch {
            bubble_position: Some(BubblePosition::At(-1200, -80)),
            ..ConfigPatch::default()
        };
        let saved_text = merge_into_ini(original, &patch).unwrap();
        // 新位置键可以追加到旧文件，已有 BOM、换行、键名、别名和时长均保留。
        assert!(saved_text.starts_with(original));
        assert!(saved_text.contains("bubble_position = -1200,-80\r\n"));
        assert!(saved_text.contains("bubble_scale = 100\r\n"));
        assert!(saved_text.ends_with("bubble_theme = green\r\n"));
        let cfg = Config::parse_ini(&saved_text);
        assert_eq!(cfg.bubble_position, BubblePosition::At(-1200, -80));
        assert_eq!(cfg.work_minutes, 37);
        assert_eq!(cfg.warn_seconds, 120);
        assert!(!cfg.reset_on_unlock);
    }

    #[test]
    fn bubble_scale_parses_valid_percent_and_ignores_invalid() {
        assert_eq!(Config::default().bubble_scale, 100);
        assert_eq!(Config::parse_ini("bubble_scale = 80\n").bubble_scale, 80);
        assert_eq!(Config::parse_ini("bubble_scale = 300\n").bubble_scale, 300);
        for value in ["abc", "999", "0", "49", "301", ""] {
            let cfg = Config::parse_ini(&format!("bubble_scale = {value}\n"));
            assert_eq!(cfg.bubble_scale, 100, "value={value}");
        }
    }

    #[test]
    fn patching_bubble_scale_saves_only_that_key() {
        let original = "work_minutes = 37\nbreak_minutes = 5\nwarn_seconds = 120\nreset_on_unlock = false\nemergency_unlock = true\nshow_bubble = true\nbubble_position = auto\nbubble_scale = 100\nbubble_theme = green\n";
        let runtime = Config::parse_ini(original);
        let patch = ConfigPatch {
            bubble_scale: Some(150),
            ..ConfigPatch::default()
        };
        let applied = runtime.apply_patch(&patch).unwrap();
        assert_eq!(applied.bubble_scale, 150);
        assert_eq!(applied.work_minutes, 37);

        let saved_text = merge_into_ini(original, &patch).unwrap();
        assert_eq!(
            saved_text,
            original.replace("bubble_scale = 100", "bubble_scale = 150")
        );
        assert_eq!(Config::parse_ini(&saved_text), applied);
    }

    #[test]
    fn resetting_bubble_position_to_auto_is_saved() {
        let original = "work_minutes = 37\nbubble_position = 1200,80\n";
        let runtime = Config::parse_ini(original);
        let patch = ConfigPatch {
            bubble_position: Some(BubblePosition::Auto),
            ..ConfigPatch::default()
        };
        let applied = runtime.apply_patch(&patch).unwrap();
        let saved_text = merge_into_ini(original, &patch).unwrap();
        assert!(saved_text.contains("bubble_position = auto\n"));
        assert_eq!(Config::parse_ini(&saved_text), applied);
    }

    #[test]
    fn patching_bubble_keeps_durations_and_saves_only_that_key() {
        let original = "work_minutes = 37\nbreak_minutes = 5\nwarn_seconds = 120\nreset_on_unlock = false\nemergency_unlock = true\nshow_bubble = true\nbubble_position = auto\n";
        let runtime = Config::parse_ini(original);
        let patch = ConfigPatch {
            show_bubble: Some(false),
            ..ConfigPatch::default()
        };
        let applied = runtime.apply_patch(&patch).unwrap();
        assert_eq!(applied.work_minutes, 37);
        assert_eq!(applied.break_minutes, 5);
        assert_eq!(applied.warn_seconds, 120);
        assert!(!applied.reset_on_unlock);
        assert!(applied.emergency_unlock);
        assert!(!applied.show_bubble);

        let saved_text = merge_into_ini(original, &patch).unwrap();
        assert!(saved_text.contains("show_bubble = false\n"));
        assert!(saved_text.contains("work_minutes = 37\n"));
        assert_eq!(Config::parse_ini(&saved_text), applied);
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

    #[test]
    fn with_times_replaces_all_three_values() {
        let cfg = Config::default().with_times(30, 10, 20).unwrap();
        assert_eq!(cfg.work_minutes, 30);
        assert_eq!(cfg.break_minutes, 10);
        assert_eq!(cfg.warn_seconds, 20);
        assert!(cfg.reset_on_unlock);
        assert!(cfg.emergency_unlock);
    }

    #[test]
    fn with_times_rejects_out_of_range_values() {
        let cfg = Config::default();
        assert!(cfg.with_times(0, 5, 60).is_err());
        assert!(cfg.with_times(601, 5, 60).is_err());
        assert!(cfg.with_times(45, 0, 60).is_err());
        assert!(cfg.with_times(45, 121, 60).is_err());
        assert!(cfg.with_times(45, 5, 601).is_err());
        assert!(cfg.with_times(45, 5, 0).is_ok());

        let err = cfg.with_times(0, 5, 60).unwrap_err();
        assert!(err.contains("工作时长"));
    }

    #[test]
    fn with_times_truncates_warning_to_work_time() {
        let cfg = Config::default().with_times(1, 5, 600).unwrap();
        assert_eq!(cfg.warn_seconds, 59);

        let cfg = Config::default().with_times(2, 5, 60).unwrap();
        assert_eq!(cfg.warn_seconds, 60);
    }

    #[test]
    fn to_ini_text_from_empty_creates_full_config() {
        let cfg = Config::default();
        let text = cfg.to_ini_text("");
        assert!(text.starts_with("# 站会儿配置文件\n"));
        for key in INI_KEYS {
            assert!(text.contains(key));
        }
        assert_eq!(Config::parse_ini(&text), cfg);
    }

    #[test]
    fn to_ini_text_keeps_comments_blank_lines_and_unknown_keys() {
        let cfg = Config::default();
        let original = "# 头部注释\n\nunknown_key = 42\nwork_minutes = 20\n";
        let text = cfg.to_ini_text(original);

        assert!(text.contains("# 头部注释"));
        assert!(text.contains("\n\n"));
        assert!(text.contains("unknown_key = 42"));
        assert!(text.contains("work_minutes = 45"));
        // 原有顺序不变：头部注释、未知键、工作时长。
        let header = text.find("# 头部注释").unwrap();
        let unknown = text.find("unknown_key").unwrap();
        let work = text.find("work_minutes = 45").unwrap();
        assert!(header < unknown && unknown < work);
        assert_eq!(Config::parse_ini(&text), cfg);
    }

    #[test]
    fn to_ini_text_preserves_trailing_comments() {
        let cfg = Config::default();
        let original = "warn_seconds = 15 ; 行尾注释\nreset_on_unlock = false # 关闭\n";
        let text = cfg.to_ini_text(original);

        assert!(text.contains("warn_seconds = 60 ; 行尾注释"));
        assert!(text.contains("reset_on_unlock = true # 关闭"));
        assert_eq!(Config::parse_ini(&text), cfg);
    }

    #[test]
    fn to_ini_text_appends_missing_keys_with_comments() {
        let cfg = Config::default();
        let text = cfg.to_ini_text("work_minutes = 30");

        assert!(text.contains("# 休息时长（分钟），1-120"));
        assert!(text.contains("break_minutes = 5"));
        assert!(text.contains("# 是否显示桌面倒计时气泡"));
        assert!(text.contains("show_bubble = true"));
        assert!(text.contains("# 气泡位置：auto（工作区右上角）或 x,y"));
        assert!(text.contains("bubble_position = auto"));
        assert!(text.contains("# 气泡大小百分比，50-300；100 为标准大小"));
        assert!(text.contains("bubble_scale = 100"));
        assert!(text.contains("# 气泡皮肤：green/blue/orange/dark"));
        assert!(text.contains("bubble_theme = green"));
        assert!(text.ends_with('\n'));
        assert_eq!(Config::parse_ini(&text), cfg);
    }

    #[test]
    fn to_ini_text_preserves_crlf() {
        let cfg = Config::default();
        let original = "# 注释\r\nwork_minutes = 20\r\n";
        let text = cfg.to_ini_text(original);

        assert!(text.starts_with("# 注释\r\n"));
        assert!(text.contains("work_minutes = 45\r\n"));
        let without_crlf = text.replace("\r\n", "");
        assert!(!without_crlf.contains('\n'));
        assert!(!without_crlf.contains('\r'));
        assert_eq!(Config::parse_ini(&text), cfg);
    }

    #[test]
    fn to_ini_text_preserves_bom() {
        let cfg = Config::default();
        let text = cfg.to_ini_text("\u{feff}work_minutes = 20\n");

        assert!(text.starts_with("\u{feff}work_minutes = 45"));
        assert_eq!(Config::parse_ini(&text), cfg);
    }

    #[test]
    fn to_ini_text_replaces_duplicate_keys() {
        let cfg = Config::default();
        let original = "work_minutes = 20\nWORK_MINUTES = 30\n  work_minutes=40  \n";
        let text = cfg.to_ini_text(original);

        assert_eq!(text.matches("work_minutes = 45").count(), 3);
        assert!(!text.contains("work_minutes = 20"));
        assert_eq!(Config::parse_ini(&text), cfg);
    }

    #[test]
    fn to_ini_text_round_trips_custom_config() {
        let cfg = Config {
            work_minutes: 1,
            break_minutes: 120,
            warn_seconds: 59,
            reset_on_unlock: false,
            emergency_unlock: false,
            show_bubble: true,
            bubble_position: BubblePosition::At(-500, 20),
            bubble_scale: 150,
            bubble_theme: BubbleTheme::Orange,
            language: LanguagePref::Auto,
        };
        let text = cfg.to_ini_text("work_minutes = 10\nunknown = value # 注释\n");
        assert!(text.contains("bubble_position = -500,20"));
        assert!(text.contains("bubble_scale = 150"));
        assert!(text.contains("bubble_theme = orange"));
        assert_eq!(Config::parse_ini(&text), cfg);
    }

    #[test]
    fn bubble_theme_parses_valid_names_and_ignores_invalid() {
        assert_eq!(Config::default().bubble_theme, BubbleTheme::Green);
        for (value, expected) in [
            ("green", BubbleTheme::Green),
            ("GREEN", BubbleTheme::Green),
            ("blue", BubbleTheme::Blue),
            ("Blue", BubbleTheme::Blue),
            ("orange", BubbleTheme::Orange),
            ("ORANGE", BubbleTheme::Orange),
            ("dark", BubbleTheme::Dark),
            (" Dark ", BubbleTheme::Dark),
        ] {
            let cfg = Config::parse_ini(&format!("bubble_theme = {value}\n"));
            assert_eq!(cfg.bubble_theme, expected, "value={value}");
        }
        for value in ["", "red", "green,blue", "1", "lime", "true"] {
            let cfg = Config::parse_ini(&format!("bubble_theme = {value}\n"));
            assert_eq!(cfg.bubble_theme, BubbleTheme::Green, "value={value}");
        }
    }

    #[test]
    fn patching_bubble_theme_saves_only_that_key() {
        let original = "work_minutes = 37\nbreak_minutes = 5\nwarn_seconds = 120\nreset_on_unlock = false\nemergency_unlock = true\nshow_bubble = true\nbubble_position = auto\nbubble_scale = 100\nbubble_theme = green\n";
        let runtime = Config::parse_ini(original);
        let patch = ConfigPatch {
            bubble_theme: Some(BubbleTheme::Blue),
            ..ConfigPatch::default()
        };
        let applied = runtime.apply_patch(&patch).unwrap();
        assert_eq!(applied.bubble_theme, BubbleTheme::Blue);
        assert_eq!(applied.work_minutes, 37);

        let saved_text = merge_into_ini(original, &patch).unwrap();
        assert_eq!(
            saved_text,
            original.replace("bubble_theme = green", "bubble_theme = blue")
        );
        assert_eq!(Config::parse_ini(&saved_text), applied);
    }

    #[test]
    fn saving_theme_upgrades_old_ini_without_rewriting_existing_settings() {
        let original = "work_minutes = 37\nbreak_minutes = 5\nwarn_seconds = 120\nreset_on_unlock = false\nemergency_unlock = true\nshow_bubble = true\nbubble_position = auto\nbubble_scale = 100\n";
        let patch = ConfigPatch {
            bubble_theme: Some(BubbleTheme::Dark),
            ..ConfigPatch::default()
        };
        let saved_text = merge_into_ini(original, &patch).unwrap();
        assert!(saved_text.starts_with(original.trim_end()));
        assert!(saved_text.contains("work_minutes = 37\n"));
        assert!(saved_text.contains("bubble_scale = 100\n"));
        assert!(saved_text.contains("# 气泡皮肤：green/blue/orange/dark\n"));
        assert!(saved_text.ends_with("bubble_theme = dark\n"));
        let cfg = Config::parse_ini(&saved_text);
        assert_eq!(cfg.bubble_theme, BubbleTheme::Dark);
        assert_eq!(cfg.work_minutes, 37);
        assert_eq!(cfg.bubble_scale, 100);
    }

    #[test]
    fn language_parses_case_insensitive_values_and_ignores_invalid() {
        assert_eq!(Config::default().language, LanguagePref::Auto);
        assert_eq!(
            Config::parse_ini("language = EN\n").language,
            LanguagePref::En
        );
        assert_eq!(
            Config::parse_ini("language = zh\n").language,
            LanguagePref::Zh
        );
        assert_eq!(
            Config::parse_ini("language = Auto\n").language,
            LanguagePref::Auto
        );
        for value in ["", "english", "cn", "jp", "1", "true", "zh-CN"] {
            let cfg = Config::parse_ini(&format!("language = {value}\n"));
            assert_eq!(cfg.language, LanguagePref::Auto, "value={value}");
        }
    }

    #[test]
    fn saving_other_settings_keeps_language_line_and_does_not_append_it() {
        let original = "work_minutes = 37\nlanguage = en\n";
        let patch = ConfigPatch {
            work_minutes: Some(30),
            ..ConfigPatch::default()
        };
        let saved = merge_into_ini(original, &patch).unwrap();
        assert!(saved.contains("work_minutes = 30\n"));
        assert!(saved.contains("language = en\n"));
        assert_eq!(Config::parse_ini(&saved).language, LanguagePref::En);

        let original = "work_minutes = 37\n";
        let saved = merge_into_ini(original, &patch).unwrap();
        assert!(!saved.to_ascii_lowercase().contains("language"));
        assert_eq!(Config::parse_ini(&saved).language, LanguagePref::Auto);
    }
}
