//! 界面语言。目前只覆盖桌面气泡和锁屏遮罩；托盘、设置窗口等仍为中文。

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

pub fn bubble_countdown(lang: Lang, clock: &str) -> String {
    match lang {
        Lang::Zh => format!("距休息 {clock}"),
        Lang::En => format!("Break in {clock}"),
    }
}

pub fn bubble_status(lang: Lang, paused: bool) -> &'static str {
    match (lang, paused) {
        (Lang::Zh, true) => "已暂停",
        (Lang::Zh, false) => "工作中",
        (Lang::En, true) => "Paused",
        (Lang::En, false) => "Working",
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
        assert_eq!(bubble_status(Lang::Zh, false), "工作中");
        assert_eq!(bubble_status(Lang::Zh, true), "已暂停");
        assert_eq!(bubble_status(Lang::En, false), "Working");
        assert_eq!(bubble_status(Lang::En, true), "Paused");
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
        assert_eq!(zh, "距休息 600:00");
        assert_eq!(en, "Break in 600:00");
        let available = bubble_text_width(140, 96);
        assert_eq!(available, 90);
        assert!(
            estimated_px(&zh) <= available,
            "zh {}px > {available}px",
            estimated_px(&zh)
        );
        assert!(
            estimated_px(&en) <= available,
            "en {}px > {available}px",
            estimated_px(&en)
        );
    }

    #[test]
    fn current_language_round_trips() {
        set_current(Lang::En);
        assert_eq!(current(), Lang::En);
        set_current(Lang::Zh);
        assert_eq!(current(), Lang::Zh);
    }
}
