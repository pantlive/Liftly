//! 界面语言：桌面气泡、锁屏遮罩以及所有 Windows 界面文案。

use std::sync::atomic::{AtomicU8, Ordering};

/// INI 中的 `language` 取值：跟随系统，或强制中文/英文。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum LanguagePref {
    #[default]
    Auto,
    Zh,
    En,
}

impl LanguagePref {
    pub fn parse(value: &str) -> Option<Self> {
        if value.eq_ignore_ascii_case("auto") {
            Some(Self::Auto)
        } else if value.eq_ignore_ascii_case("zh") {
            Some(Self::Zh)
        } else if value.eq_ignore_ascii_case("en") {
            Some(Self::En)
        } else {
            None
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Zh => "zh",
            Self::En => "en",
        }
    }
}

/// 实际使用的界面语言。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Lang {
    Zh = 0,
    En = 1,
}

static CURRENT: AtomicU8 = AtomicU8::new(Lang::Zh as u8);

/// 把偏好和 Windows UI 语言（`LANGID`）解析成实际语言。
/// `auto` 时取 `langid` 的低 10 位：`0x04`（中文）为中文，其余为英文。
pub fn resolve(pref: LanguagePref, system_langid: u16) -> Lang {
    match pref {
        LanguagePref::Zh => Lang::Zh,
        LanguagePref::En => Lang::En,
        LanguagePref::Auto => {
            if system_langid & 0x3FF == 0x04 {
                Lang::Zh
            } else {
                Lang::En
            }
        }
    }
}

pub fn set_current(lang: Lang) {
    CURRENT.store(lang as u8, Ordering::SeqCst);
}

pub fn current() -> Lang {
    if CURRENT.load(Ordering::SeqCst) == Lang::En as u8 {
        Lang::En
    } else {
        Lang::Zh
    }
}

/// 不依赖当前语言状态的静态界面文案。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Msg {
    AppName,
    TrayBreakNow,
    TrayResume,
    TrayPause,
    TrayRestart,
    TrayShowBubble,
    TraySettings,
    TrayExit,
    TrayWorkDuration,
    TrayBreakDuration,
    TrayWarnBeforeLock,
    TrayUnitMinutes,
    TrayUnitSeconds,
    TrayNoWarning,
    TrayCustom,
    TrayBubbleSize,
    TrayBubbleTheme,
    TipPaused,
    TipBreaking,
    StartupTitle,
    BreakOver,
    SaveFailed,
    TrayCreateFailed,
    StartupBalloonFailed,
    Panic,
    AlreadyRunning,
    PopupTitleLine,
    PopupStartingNow,
    PopupHint,
    SettingsTitle,
    SettingsWorkLabel,
    SettingsBreakLabel,
    SettingsWarnLabel,
    SettingsWorkRange,
    SettingsBreakRange,
    SettingsWarnRange,
    SettingsReset,
    SettingsEmergency,
    SettingsAutostart,
    SettingsLanguage,
    SettingsLanguageAuto,
    SettingsLanguageZh,
    SettingsLanguageEn,
    SettingsSave,
    SettingsCancel,
    SettingsErrorTitle,
    SettingsNotANumber,
}

/// 静态文案中英对照。调用方显式传入语言，测试不依赖全局 `current()`。
pub const fn tr(lang: Lang, msg: Msg) -> &'static str {
    match (lang, msg) {
        (Lang::Zh, Msg::AppName) => "站会儿",
        (Lang::En, Msg::AppName) => "Liftly",
        (Lang::Zh, Msg::TrayBreakNow) => "立即休息",
        (Lang::En, Msg::TrayBreakNow) => "Take a break now",
        (Lang::Zh, Msg::TrayResume) => "继续计时",
        (Lang::En, Msg::TrayResume) => "Resume timer",
        (Lang::Zh, Msg::TrayPause) => "暂停计时",
        (Lang::En, Msg::TrayPause) => "Pause timer",
        (Lang::Zh, Msg::TrayRestart) => "重新开始计时",
        (Lang::En, Msg::TrayRestart) => "Restart timer",
        (Lang::Zh, Msg::TrayShowBubble) => "显示桌面倒计时",
        (Lang::En, Msg::TrayShowBubble) => "Show desktop countdown",
        (Lang::Zh, Msg::TraySettings) => "设置…",
        (Lang::En, Msg::TraySettings) => "Settings…",
        (Lang::Zh, Msg::TrayExit) => "退出",
        (Lang::En, Msg::TrayExit) => "Exit",
        (Lang::Zh, Msg::TrayWorkDuration) => "工作时长",
        (Lang::En, Msg::TrayWorkDuration) => "Work duration",
        (Lang::Zh, Msg::TrayBreakDuration) => "休息时长",
        (Lang::En, Msg::TrayBreakDuration) => "Break duration",
        (Lang::Zh, Msg::TrayWarnBeforeLock) => "锁定前提醒",
        (Lang::En, Msg::TrayWarnBeforeLock) => "Warn before lock",
        (Lang::Zh, Msg::TrayUnitMinutes) => "分钟",
        (Lang::En, Msg::TrayUnitMinutes) => "min",
        (Lang::Zh, Msg::TrayUnitSeconds) => "秒",
        (Lang::En, Msg::TrayUnitSeconds) => "s",
        (Lang::Zh, Msg::TrayNoWarning) => "不提醒",
        (Lang::En, Msg::TrayNoWarning) => "No warning",
        (Lang::Zh, Msg::TrayCustom) => "自定义…",
        (Lang::En, Msg::TrayCustom) => "Custom…",
        (Lang::Zh, Msg::TrayBubbleSize) => "气泡大小",
        (Lang::En, Msg::TrayBubbleSize) => "Bubble size",
        (Lang::Zh, Msg::TrayBubbleTheme) => "气泡皮肤",
        (Lang::En, Msg::TrayBubbleTheme) => "Bubble style",
        (Lang::Zh, Msg::TipPaused) => "站会儿：已暂停",
        (Lang::En, Msg::TipPaused) => "Liftly: paused",
        (Lang::Zh, Msg::TipBreaking) => "站会儿：休息中",
        (Lang::En, Msg::TipBreaking) => "Liftly: on break",
        (Lang::Zh, Msg::StartupTitle) => "站会儿已启动",
        (Lang::En, Msg::StartupTitle) => "Liftly started",
        (Lang::Zh, Msg::BreakOver) => "休息结束，继续加油！",
        (Lang::En, Msg::BreakOver) => "Break over. Keep it up!",
        (Lang::Zh, Msg::SaveFailed) => "已生效，但没能保存到 liftly.ini",
        (Lang::En, Msg::SaveFailed) => "Applied, but failed to save liftly.ini",
        (Lang::Zh, Msg::TrayCreateFailed) => {
            "无法创建托盘图标，程序不能继续运行。\n请确认任务栏通知区域可用后重新启动站会儿。"
        }
        (Lang::En, Msg::TrayCreateFailed) => {
            "Could not create the tray icon, so Liftly cannot continue.\nMake sure the notification area is available, then restart Liftly."
        }
        (Lang::Zh, Msg::StartupBalloonFailed) => {
            "启动提示气泡显示失败，但程序已经在托盘中运行。\n请检查任务栏通知区域设置。"
        }
        (Lang::En, Msg::StartupBalloonFailed) => {
            "The startup notification could not be shown, but Liftly is running in the tray.\nCheck the notification-area settings."
        }
        (Lang::Zh, Msg::Panic) => {
            "程序内部错误，即将退出。请重新启动站会儿；若问题持续出现，请查看 README 的常见问题。"
        }
        (Lang::En, Msg::Panic) => {
            "An internal error occurred and Liftly will exit. Restart it; if the problem persists, see the README FAQ."
        }
        (Lang::Zh, Msg::AlreadyRunning) => "站会儿已经在运行",
        (Lang::En, Msg::AlreadyRunning) => "Liftly is already running",
        (Lang::Zh, Msg::PopupTitleLine) => "站会儿 · 即将休息",
        (Lang::En, Msg::PopupTitleLine) => "Liftly · Break soon",
        (Lang::Zh, Msg::PopupStartingNow) => "马上开始休息",
        (Lang::En, Msg::PopupStartingNow) => "Break starting now",
        (Lang::Zh, Msg::PopupHint) => "请保存手头工作 · 点击关闭此提示",
        (Lang::En, Msg::PopupHint) => "Save your work · Click to dismiss",
        (Lang::Zh, Msg::SettingsTitle) => "站会儿设置",
        (Lang::En, Msg::SettingsTitle) => "Liftly Settings",
        (Lang::Zh, Msg::SettingsWorkLabel) => "工作时长（分钟）",
        (Lang::En, Msg::SettingsWorkLabel) => "Work duration (min)",
        (Lang::Zh, Msg::SettingsBreakLabel) => "休息时长（分钟）",
        (Lang::En, Msg::SettingsBreakLabel) => "Break duration (min)",
        (Lang::Zh, Msg::SettingsWarnLabel) => "锁定前提醒（秒）",
        (Lang::En, Msg::SettingsWarnLabel) => "Warn before lock (s)",
        (Lang::Zh, Msg::SettingsWorkRange) => "1–600 分钟",
        (Lang::En, Msg::SettingsWorkRange) => "1–600 min",
        (Lang::Zh, Msg::SettingsBreakRange) => "1–120 分钟",
        (Lang::En, Msg::SettingsBreakRange) => "1–120 min",
        (Lang::Zh, Msg::SettingsWarnRange) => "0–600 秒，0 表示不提醒",
        (Lang::En, Msg::SettingsWarnRange) => "0–600 s; 0 = no warning",
        (Lang::Zh, Msg::SettingsReset) => "解锁后重新计时",
        (Lang::En, Msg::SettingsReset) => "Restart after unlock",
        (Lang::Zh, Msg::SettingsEmergency) => "紧急解锁",
        (Lang::En, Msg::SettingsEmergency) => "Emergency unlock",
        (Lang::Zh, Msg::SettingsAutostart) => "开机自启动",
        (Lang::En, Msg::SettingsAutostart) => "Start with Windows",
        (Lang::Zh, Msg::SettingsLanguage) => "界面语言",
        (Lang::En, Msg::SettingsLanguage) => "Language",
        (Lang::Zh, Msg::SettingsLanguageAuto) => "跟随系统",
        (Lang::En, Msg::SettingsLanguageAuto) => "System default",
        (Lang::Zh, Msg::SettingsLanguageZh) => "中文",
        (Lang::En, Msg::SettingsLanguageZh) => "Chinese",
        (_, Msg::SettingsLanguageEn) => "English",
        (Lang::Zh, Msg::SettingsSave) => "保存",
        (Lang::En, Msg::SettingsSave) => "Save",
        (Lang::Zh, Msg::SettingsCancel) => "取消",
        (Lang::En, Msg::SettingsCancel) => "Cancel",
        (Lang::Zh, Msg::SettingsErrorTitle) => "设置提示",
        (Lang::En, Msg::SettingsErrorTitle) => "Settings",
        (Lang::Zh, Msg::SettingsNotANumber) => "请输入数字",
        (Lang::En, Msg::SettingsNotANumber) => "Please enter a number",
    }
}

/// 托盘菜单的时长预设文案，`seconds` 为真时使用秒。
pub fn duration_preset(lang: Lang, value: u32, seconds: bool) -> String {
    let unit = if seconds {
        tr(lang, Msg::TrayUnitSeconds)
    } else {
        tr(lang, Msg::TrayUnitMinutes)
    };
    format!("{value} {unit}")
}

/// 托盘菜单的自定义时长项；`current` 不在预设中时使用。
pub fn duration_custom(lang: Lang, current: u32, seconds: bool) -> String {
    let unit = if seconds {
        tr(lang, Msg::TrayUnitSeconds)
    } else {
        tr(lang, Msg::TrayUnitMinutes)
    };
    match lang {
        Lang::Zh => format!("自定义（{current} {unit}）"),
        Lang::En => format!("Custom ({current} {unit})"),
    }
}

/// 托盘菜单中非预设气泡大小的置灰项。
pub fn bubble_scale_custom(lang: Lang, percent: u32) -> String {
    match lang {
        Lang::Zh => format!("当前（{percent}%，自定义）"),
        Lang::En => format!("Current ({percent}%, custom)"),
    }
}

/// 气泡大小五档名称。
pub const fn bubble_scale_labels(lang: Lang) -> [&'static str; 5] {
    match lang {
        Lang::Zh => [
            "小（80%）",
            "标准（100%）",
            "大（125%）",
            "较大（150%）",
            "特大（200%）",
        ],
        Lang::En => [
            "Small (80%)",
            "Standard (100%)",
            "Large (125%)",
            "Larger (150%)",
            "Extra large (200%)",
        ],
    }
}

/// 气泡皮肤名称。
pub const fn bubble_theme_labels(lang: Lang) -> [&'static str; 4] {
    match lang {
        Lang::Zh => ["清新绿", "天空蓝", "暖橙", "深色"],
        Lang::En => ["Fresh green", "Sky blue", "Warm orange", "Dark"],
    }
}

pub fn tray_tip_warning(lang: Lang, secs: u64) -> String {
    match lang {
        Lang::Zh => format!("站会儿：{secs} 秒后休息"),
        Lang::En => format!("Liftly: break in {secs} s"),
    }
}

pub fn tray_tip_work(lang: Lang, minutes: u64) -> String {
    match lang {
        Lang::Zh => format!("站会儿：距离休息还有 {minutes} 分钟"),
        Lang::En => format!("Liftly: {minutes} min until break"),
    }
}

pub fn warn_balloon(lang: Lang, secs: u64) -> String {
    match lang {
        Lang::Zh => format!("还有 {secs} 秒就要站起来活动啦，请保存手头工作。"),
        Lang::En => format!("Get ready to stand up in {secs} s. Save your work."),
    }
}

pub fn startup_balloon(lang: Lang, work_minutes: u32, break_minutes: u32) -> String {
    match lang {
        Lang::Zh => format!(
            "已开始计时：工作 {work_minutes} 分钟，休息 {break_minutes} 分钟。\n右键托盘图标可设置，双击打开设置。"
        ),
        Lang::En => format!(
            "Timing started: work {work_minutes} min, break {break_minutes} min.\nRight-click the tray icon to configure, or double-click to open Settings."
        ),
    }
}

pub fn create_main_window_failed(lang: Lang, code: u32) -> String {
    match lang {
        Lang::Zh => format!("创建主窗口失败（错误码 {code}），程序无法启动。"),
        Lang::En => {
            format!("Failed to create the main window (error {code}). Liftly cannot start.")
        }
    }
}

pub fn register_class_failed(lang: Lang, code: u32) -> String {
    match lang {
        Lang::Zh => format!("注册主窗口类失败（错误码 {code}），程序无法启动。"),
        Lang::En => {
            format!("Failed to register the main window class (error {code}). Liftly cannot start.")
        }
    }
}

pub fn popup_countdown(lang: Lang, secs: u64) -> String {
    match lang {
        Lang::Zh => format!("{secs} 秒后锁定，站起来活动一下"),
        // 英文在 360px 宽的弹窗里保持短句，避免被省略号截断。
        Lang::En => format!("Locking in {secs} s"),
    }
}

pub fn settings_autostart_failed(lang: Lang, message: &str) -> String {
    match lang {
        Lang::Zh => format!("开机自启动设置失败：{message}"),
        Lang::En => format!("Failed to update Start with Windows: {message}"),
    }
}

pub fn autostart_path_error(lang: Lang) -> &'static str {
    match lang {
        Lang::Zh => "无法读取程序路径",
        Lang::En => "Could not read the program path",
    }
}

pub fn autostart_write_error(lang: Lang, status: u32) -> String {
    match lang {
        Lang::Zh => format!("无法写入开机自启动（错误码 {status}）"),
        Lang::En => format!("Could not enable Start with Windows (error {status})"),
    }
}

pub fn autostart_open_key_error(lang: Lang, status: u32) -> String {
    match lang {
        Lang::Zh => format!("无法打开注册表项（错误码 {status}）"),
        Lang::En => format!("Could not open the registry key (error {status})"),
    }
}

pub fn autostart_delete_error(lang: Lang, status: u32) -> String {
    match lang {
        Lang::Zh => format!("无法删除开机自启动（错误码 {status}）"),
        Lang::En => format!("Could not disable Start with Windows (error {status})"),
    }
}

pub fn autostart_create_key_error(lang: Lang, status: u32) -> String {
    match lang {
        Lang::Zh => format!("无法创建注册表项（错误码 {status}）"),
        Lang::En => format!("Could not create the registry key (error {status})"),
    }
}

pub fn work_range_error(lang: Lang, min: u32, max: u32) -> String {
    match lang {
        Lang::Zh => format!("工作时长必须在 {min} 到 {max} 分钟之间"),
        Lang::En => format!("Work duration must be between {min} and {max} minutes"),
    }
}

pub fn break_range_error(lang: Lang, min: u32, max: u32) -> String {
    match lang {
        Lang::Zh => format!("休息时长必须在 {min} 到 {max} 分钟之间"),
        Lang::En => format!("Break duration must be between {min} and {max} minutes"),
    }
}

pub fn warn_range_error(lang: Lang, max: u32) -> String {
    match lang {
        Lang::Zh => format!("提醒秒数必须在 0 到 {max} 秒之间"),
        Lang::En => format!("Warn seconds must be between 0 and {max}"),
    }
}

pub fn bubble_scale_range_error(lang: Lang, min: u32, max: u32) -> String {
    match lang {
        Lang::Zh => format!("气泡大小必须在 {min}% 到 {max}% 之间"),
        Lang::En => format!("Bubble size must be between {min}% and {max}%"),
    }
}

pub fn bubble_countdown(lang: Lang, clock: &str) -> String {
    match lang {
        Lang::Zh => format!("距休息 {clock}"),
        Lang::En => format!("Break in {clock}"),
    }
}

pub fn bubble_paused(lang: Lang, clock: &str) -> String {
    match lang {
        Lang::Zh => format!("已暂停 {clock}"),
        Lang::En => format!("Paused {clock}"),
    }
}

pub fn overlay_title(lang: Lang) -> &'static str {
    match lang {
        Lang::Zh => "站起来，活动一下！",
        Lang::En => "Time to stand up and move!",
    }
}

pub fn overlay_countdown(lang: Lang, remaining_ms: u64) -> String {
    let clock = crate::view::format_clock(remaining_ms);
    match lang {
        Lang::Zh => format!("剩余 {clock}"),
        Lang::En => format!("Time left {clock}"),
    }
}

pub fn overlay_hint(lang: Lang, emergency: bool) -> &'static str {
    match (lang, emergency) {
        (Lang::Zh, true) => "时间到后自动解锁 · 紧急解锁：Ctrl+Alt+Shift+F12",
        (Lang::Zh, false) => "时间到后自动解锁",
        (Lang::En, true) => {
            "Unlocks automatically when time is up · Emergency unlock: Ctrl+Alt+Shift+F12"
        }
        (Lang::En, false) => "Unlocks automatically when time is up",
    }
}

pub fn overlay_window_title(lang: Lang) -> &'static str {
    match lang {
        Lang::Zh => "站会儿",
        Lang::En => "Liftly",
    }
}

pub fn bubble_window_title(lang: Lang) -> &'static str {
    match lang {
        Lang::Zh => "站会儿倒计时",
        Lang::En => "Liftly countdown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view::bubble_text_width;

    #[test]
    fn resolve_follows_chinese_primary_langid_unless_overridden() {
        assert_eq!(resolve(LanguagePref::Auto, 0x0804), Lang::Zh);
        assert_eq!(resolve(LanguagePref::Auto, 0x0404), Lang::Zh);
        assert_eq!(resolve(LanguagePref::Auto, 0x0409), Lang::En);
        assert_eq!(resolve(LanguagePref::Auto, 0x0411), Lang::En);
        assert_eq!(resolve(LanguagePref::Zh, 0x0409), Lang::Zh);
        assert_eq!(resolve(LanguagePref::En, 0x0804), Lang::En);
        assert_eq!(resolve(LanguagePref::En, 0x0404), Lang::En);
        assert_eq!(resolve(LanguagePref::Zh, 0x0411), Lang::Zh);
    }

    #[test]
    fn language_pref_parses_case_insensitively_and_rejects_invalid() {
        assert_eq!(LanguagePref::parse("auto"), Some(LanguagePref::Auto));
        assert_eq!(LanguagePref::parse("AUTO"), Some(LanguagePref::Auto));
        assert_eq!(LanguagePref::parse("zh"), Some(LanguagePref::Zh));
        assert_eq!(LanguagePref::parse("ZH"), Some(LanguagePref::Zh));
        assert_eq!(LanguagePref::parse("en"), Some(LanguagePref::En));
        assert_eq!(LanguagePref::parse("En"), Some(LanguagePref::En));
        for value in ["", "english", "cn", "jp", "1", "true", "zh-CN", "en-US"] {
            assert_eq!(LanguagePref::parse(value), None, "value={value}");
        }
        assert_eq!(LanguagePref::Auto.as_str(), "auto");
        assert_eq!(LanguagePref::Zh.as_str(), "zh");
        assert_eq!(LanguagePref::En.as_str(), "en");
    }

    #[test]
    fn copy_matches_chinese_original_and_english_translations() {
        assert_eq!(bubble_countdown(Lang::Zh, "45:00"), "距休息 45:00");
        assert_eq!(bubble_countdown(Lang::En, "45:00"), "Break in 45:00");
        assert_eq!(bubble_paused(Lang::Zh, "45:00"), "已暂停 45:00");
        assert_eq!(bubble_paused(Lang::En, "45:00"), "Paused 45:00");
        assert_eq!(overlay_title(Lang::Zh), "站起来，活动一下！");
        assert_eq!(overlay_title(Lang::En), "Time to stand up and move!");
        assert_eq!(overlay_countdown(Lang::Zh, 999), "剩余 00:01");
        assert_eq!(overlay_countdown(Lang::En, 999), "Time left 00:01");
        assert_eq!(overlay_countdown(Lang::Zh, 0), "剩余 00:00");
        assert_eq!(overlay_countdown(Lang::En, 5 * 60_000), "Time left 05:00");
        assert_eq!(
            overlay_hint(Lang::Zh, true),
            "时间到后自动解锁 · 紧急解锁：Ctrl+Alt+Shift+F12"
        );
        assert_eq!(overlay_hint(Lang::Zh, false), "时间到后自动解锁");
        assert_eq!(
            overlay_hint(Lang::En, true),
            "Unlocks automatically when time is up · Emergency unlock: Ctrl+Alt+Shift+F12"
        );
        assert_eq!(
            overlay_hint(Lang::En, false),
            "Unlocks automatically when time is up"
        );
        assert_eq!(overlay_window_title(Lang::Zh), "站会儿");
        assert_eq!(overlay_window_title(Lang::En), "Liftly");
        assert_eq!(bubble_window_title(Lang::Zh), "站会儿倒计时");
        assert_eq!(bubble_window_title(Lang::En), "Liftly countdown");
    }

    fn estimated_px(text: &str) -> i32 {
        text.chars()
            .map(|c| if u32::from(c) <= 0x7F { 6 } else { 13 })
            .sum()
    }

    #[test]
    fn english_longest_countdown_fits_compact_bubble_text_width() {
        let zh = bubble_countdown(Lang::Zh, "600:00");
        let en = bubble_countdown(Lang::En, "600:00");
        let zh_paused = bubble_paused(Lang::Zh, "600:00");
        let en_paused = bubble_paused(Lang::En, "600:00");
        assert_eq!(zh, "距休息 600:00");
        assert_eq!(en, "Break in 600:00");
        assert_eq!(zh_paused, "已暂停 600:00");
        assert_eq!(en_paused, "Paused 600:00");
        let available = bubble_text_width(134, 96);
        assert_eq!(available, 90);
        for text in [&zh, &en, &zh_paused, &en_paused] {
            assert!(
                estimated_px(text) <= available,
                "{text} {}px > {available}px",
                estimated_px(text)
            );
        }
    }

    const STATIC_COPY: &[(Msg, &str, &str)] = &[
        (Msg::AppName, "站会儿", "Liftly"),
        (Msg::TrayBreakNow, "立即休息", "Take a break now"),
        (Msg::TrayResume, "继续计时", "Resume timer"),
        (Msg::TrayPause, "暂停计时", "Pause timer"),
        (Msg::TrayRestart, "重新开始计时", "Restart timer"),
        (Msg::TrayShowBubble, "显示桌面倒计时", "Show desktop countdown"),
        (Msg::TraySettings, "设置…", "Settings…"),
        (Msg::TrayExit, "退出", "Exit"),
        (Msg::TrayWorkDuration, "工作时长", "Work duration"),
        (Msg::TrayBreakDuration, "休息时长", "Break duration"),
        (Msg::TrayWarnBeforeLock, "锁定前提醒", "Warn before lock"),
        (Msg::TrayUnitMinutes, "分钟", "min"),
        (Msg::TrayUnitSeconds, "秒", "s"),
        (Msg::TrayNoWarning, "不提醒", "No warning"),
        (Msg::TrayCustom, "自定义…", "Custom…"),
        (Msg::TrayBubbleSize, "气泡大小", "Bubble size"),
        (Msg::TrayBubbleTheme, "气泡皮肤", "Bubble style"),
        (Msg::TipPaused, "站会儿：已暂停", "Liftly: paused"),
        (Msg::TipBreaking, "站会儿：休息中", "Liftly: on break"),
        (Msg::StartupTitle, "站会儿已启动", "Liftly started"),
        (Msg::BreakOver, "休息结束，继续加油！", "Break over. Keep it up!"),
        (Msg::SaveFailed, "已生效，但没能保存到 liftly.ini", "Applied, but failed to save liftly.ini"),
        (
            Msg::TrayCreateFailed,
            "无法创建托盘图标，程序不能继续运行。\n请确认任务栏通知区域可用后重新启动站会儿。",
            "Could not create the tray icon, so Liftly cannot continue.\nMake sure the notification area is available, then restart Liftly.",
        ),
        (
            Msg::StartupBalloonFailed,
            "启动提示气泡显示失败，但程序已经在托盘中运行。\n请检查任务栏通知区域设置。",
            "The startup notification could not be shown, but Liftly is running in the tray.\nCheck the notification-area settings.",
        ),
        (
            Msg::Panic,
            "程序内部错误，即将退出。请重新启动站会儿；若问题持续出现，请查看 README 的常见问题。",
            "An internal error occurred and Liftly will exit. Restart it; if the problem persists, see the README FAQ.",
        ),
        (Msg::AlreadyRunning, "站会儿已经在运行", "Liftly is already running"),
        (Msg::PopupTitleLine, "站会儿 · 即将休息", "Liftly · Break soon"),
        (Msg::PopupStartingNow, "马上开始休息", "Break starting now"),
        (Msg::PopupHint, "请保存手头工作 · 点击关闭此提示", "Save your work · Click to dismiss"),
        (Msg::SettingsTitle, "站会儿设置", "Liftly Settings"),
        (Msg::SettingsWorkLabel, "工作时长（分钟）", "Work duration (min)"),
        (Msg::SettingsBreakLabel, "休息时长（分钟）", "Break duration (min)"),
        (Msg::SettingsWarnLabel, "锁定前提醒（秒）", "Warn before lock (s)"),
        (Msg::SettingsWorkRange, "1–600 分钟", "1–600 min"),
        (Msg::SettingsBreakRange, "1–120 分钟", "1–120 min"),
        (Msg::SettingsWarnRange, "0–600 秒，0 表示不提醒", "0–600 s; 0 = no warning"),
        (Msg::SettingsReset, "解锁后重新计时", "Restart after unlock"),
        (Msg::SettingsEmergency, "紧急解锁", "Emergency unlock"),
        (Msg::SettingsAutostart, "开机自启动", "Start with Windows"),
        (Msg::SettingsLanguage, "界面语言", "Language"),
        (Msg::SettingsLanguageAuto, "跟随系统", "System default"),
        (Msg::SettingsLanguageZh, "中文", "Chinese"),
        (Msg::SettingsLanguageEn, "English", "English"),
        (Msg::SettingsSave, "保存", "Save"),
        (Msg::SettingsCancel, "取消", "Cancel"),
        (Msg::SettingsErrorTitle, "设置提示", "Settings"),
        (Msg::SettingsNotANumber, "请输入数字", "Please enter a number"),
    ];

    fn has_cjk(text: &str) -> bool {
        text.chars()
            .any(|character| ('\u{4e00}'..='\u{9fff}').contains(&character))
    }

    #[test]
    fn every_static_message_matches_chinese_and_english_copy() {
        for &(msg, zh, en) in STATIC_COPY {
            assert_eq!(tr(Lang::Zh, msg), zh, "{msg:?}");
            assert_eq!(tr(Lang::En, msg), en, "{msg:?}");
            assert!(
                !has_cjk(en),
                "English copy for {msg:?} contains Chinese: {en}"
            );
        }
    }

    #[test]
    fn dynamic_copy_matches_chinese_and_english_translations() {
        assert_eq!(duration_preset(Lang::Zh, 25, false), "25 分钟");
        assert_eq!(duration_preset(Lang::En, 25, false), "25 min");
        assert_eq!(duration_preset(Lang::Zh, 30, true), "30 秒");
        assert_eq!(duration_preset(Lang::En, 30, true), "30 s");
        assert_eq!(duration_custom(Lang::Zh, 37, false), "自定义（37 分钟）");
        assert_eq!(duration_custom(Lang::En, 37, false), "Custom (37 min)");
        assert_eq!(duration_custom(Lang::Zh, 45, true), "自定义（45 秒）");
        assert_eq!(duration_custom(Lang::En, 45, true), "Custom (45 s)");
        assert_eq!(bubble_scale_custom(Lang::Zh, 110), "当前（110%，自定义）");
        assert_eq!(bubble_scale_custom(Lang::En, 110), "Current (110%, custom)");
        assert_eq!(
            bubble_scale_labels(Lang::Zh),
            [
                "小（80%）",
                "标准（100%）",
                "大（125%）",
                "较大（150%）",
                "特大（200%）"
            ]
        );
        assert_eq!(
            bubble_scale_labels(Lang::En),
            [
                "Small (80%)",
                "Standard (100%)",
                "Large (125%)",
                "Larger (150%)",
                "Extra large (200%)"
            ]
        );
        assert_eq!(
            bubble_theme_labels(Lang::Zh),
            ["清新绿", "天空蓝", "暖橙", "深色"]
        );
        assert_eq!(
            bubble_theme_labels(Lang::En),
            ["Fresh green", "Sky blue", "Warm orange", "Dark"]
        );
        assert_eq!(tray_tip_warning(Lang::Zh, 59), "站会儿：59 秒后休息");
        assert_eq!(tray_tip_warning(Lang::En, 59), "Liftly: break in 59 s");
        assert_eq!(tray_tip_work(Lang::Zh, 45), "站会儿：距离休息还有 45 分钟");
        assert_eq!(tray_tip_work(Lang::En, 45), "Liftly: 45 min until break");
        assert_eq!(
            warn_balloon(Lang::Zh, 10),
            "还有 10 秒就要站起来活动啦，请保存手头工作。"
        );
        assert_eq!(
            warn_balloon(Lang::En, 10),
            "Get ready to stand up in 10 s. Save your work."
        );
        assert_eq!(
            startup_balloon(Lang::Zh, 45, 5),
            "已开始计时：工作 45 分钟，休息 5 分钟。\n右键托盘图标可设置，双击打开设置。"
        );
        assert_eq!(
            startup_balloon(Lang::En, 45, 5),
            "Timing started: work 45 min, break 5 min.\nRight-click the tray icon to configure, or double-click to open Settings."
        );
        assert_eq!(
            create_main_window_failed(Lang::Zh, 5),
            "创建主窗口失败（错误码 5），程序无法启动。"
        );
        assert_eq!(
            create_main_window_failed(Lang::En, 5),
            "Failed to create the main window (error 5). Liftly cannot start."
        );
        assert_eq!(
            register_class_failed(Lang::Zh, 6),
            "注册主窗口类失败（错误码 6），程序无法启动。"
        );
        assert_eq!(
            register_class_failed(Lang::En, 6),
            "Failed to register the main window class (error 6). Liftly cannot start."
        );
        assert_eq!(popup_countdown(Lang::Zh, 59), "59 秒后锁定，站起来活动一下");
        assert_eq!(popup_countdown(Lang::En, 59), "Locking in 59 s");
        assert_eq!(
            settings_autostart_failed(Lang::Zh, "denied"),
            "开机自启动设置失败：denied"
        );
        assert_eq!(
            settings_autostart_failed(Lang::En, "denied"),
            "Failed to update Start with Windows: denied"
        );
        assert_eq!(autostart_path_error(Lang::Zh), "无法读取程序路径");
        assert_eq!(
            autostart_path_error(Lang::En),
            "Could not read the program path"
        );
        assert_eq!(
            autostart_write_error(Lang::Zh, 5),
            "无法写入开机自启动（错误码 5）"
        );
        assert_eq!(
            autostart_write_error(Lang::En, 5),
            "Could not enable Start with Windows (error 5)"
        );
        assert_eq!(
            autostart_open_key_error(Lang::Zh, 5),
            "无法打开注册表项（错误码 5）"
        );
        assert_eq!(
            autostart_open_key_error(Lang::En, 5),
            "Could not open the registry key (error 5)"
        );
        assert_eq!(
            autostart_delete_error(Lang::Zh, 5),
            "无法删除开机自启动（错误码 5）"
        );
        assert_eq!(
            autostart_delete_error(Lang::En, 5),
            "Could not disable Start with Windows (error 5)"
        );
        assert_eq!(
            autostart_create_key_error(Lang::Zh, 5),
            "无法创建注册表项（错误码 5）"
        );
        assert_eq!(
            autostart_create_key_error(Lang::En, 5),
            "Could not create the registry key (error 5)"
        );
        assert_eq!(
            work_range_error(Lang::Zh, 1, 600),
            "工作时长必须在 1 到 600 分钟之间"
        );
        assert_eq!(
            work_range_error(Lang::En, 1, 600),
            "Work duration must be between 1 and 600 minutes"
        );
        assert_eq!(
            break_range_error(Lang::Zh, 1, 120),
            "休息时长必须在 1 到 120 分钟之间"
        );
        assert_eq!(
            break_range_error(Lang::En, 1, 120),
            "Break duration must be between 1 and 120 minutes"
        );
        assert_eq!(
            warn_range_error(Lang::Zh, 600),
            "提醒秒数必须在 0 到 600 秒之间"
        );
        assert_eq!(
            warn_range_error(Lang::En, 600),
            "Warn seconds must be between 0 and 600"
        );
        assert_eq!(
            bubble_scale_range_error(Lang::Zh, 50, 300),
            "气泡大小必须在 50% 到 300% 之间"
        );
        assert_eq!(
            bubble_scale_range_error(Lang::En, 50, 300),
            "Bubble size must be between 50% and 300%"
        );

        for text in [
            duration_preset(Lang::En, 25, false),
            duration_custom(Lang::En, 37, false),
            bubble_scale_custom(Lang::En, 110),
            bubble_scale_labels(Lang::En).join(" "),
            bubble_theme_labels(Lang::En).join(" "),
            tray_tip_warning(Lang::En, 59),
            tray_tip_work(Lang::En, 45),
            warn_balloon(Lang::En, 10),
            startup_balloon(Lang::En, 45, 5),
            create_main_window_failed(Lang::En, 5),
            register_class_failed(Lang::En, 5),
            popup_countdown(Lang::En, 59),
            settings_autostart_failed(Lang::En, "denied"),
            autostart_write_error(Lang::En, 5),
            autostart_open_key_error(Lang::En, 5),
            autostart_delete_error(Lang::En, 5),
            autostart_create_key_error(Lang::En, 5),
            work_range_error(Lang::En, 1, 600),
            break_range_error(Lang::En, 1, 120),
            warn_range_error(Lang::En, 600),
            bubble_scale_range_error(Lang::En, 50, 300),
        ] {
            assert!(!has_cjk(&text), "English copy contains Chinese: {text}");
        }
    }

    #[test]
    fn english_popup_countdown_fits_360px_window() {
        // 弹窗左右各留 16px 内边距；22px 字体按每字符约 12px 估算。
        let available = 360 - 2 * 16;
        let text = popup_countdown(Lang::En, 600);
        assert!(text.chars().count() as i32 * 12 <= available, "{text}");
    }

    #[test]
    fn current_language_round_trips() {
        set_current(Lang::En);
        assert_eq!(current(), Lang::En);
        set_current(Lang::Zh);
        assert_eq!(current(), Lang::Zh);
    }
}
