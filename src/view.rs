//! 仅包含可跨平台测试的展示逻辑，供 Windows 倒计时气泡使用。

use crate::schedule::Phase;

/// 把剩余毫秒格式化为倒计时文本。不足一秒按一秒显示，避免还没到点就出现 `00:00`。
pub fn format_clock(remaining_ms: u64) -> String {
    let total_seconds = remaining_ms.div_ceil(1_000);
    format!("{:02}:{:02}", total_seconds / 60, total_seconds % 60)
}

/// 倒计时气泡只在正常工作和手动暂停时显示；休息、锁屏或用户关闭时不显示。
pub fn bubble_visible(phase: Phase, enabled: bool, locked: bool) -> bool {
    enabled && !locked && matches!(phase, Phase::Working | Phase::Paused)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_rounds_up_and_handles_boundaries() {
        for (remaining, expected) in [
            (0, "00:00"),
            (1, "00:01"),
            (999, "00:01"),
            (1_000, "00:01"),
            (1_001, "00:02"),
            (59_000, "00:59"),
            (59_001, "01:00"),
            (60_000, "01:00"),
            (3_599_999, "60:00"),
            (3_600_000, "60:00"),
            (36_000_000, "600:00"),
        ] {
            assert_eq!(format_clock(remaining), expected, "remaining={remaining}");
        }
    }

    #[test]
    fn bubble_is_visible_only_while_working_or_paused_and_unlocked() {
        for phase in [Phase::Working, Phase::Breaking, Phase::Paused] {
            for enabled in [false, true] {
                for locked in [false, true] {
                    let expected =
                        enabled && !locked && matches!(phase, Phase::Working | Phase::Paused);
                    assert_eq!(
                        bubble_visible(phase, enabled, locked),
                        expected,
                        "phase={phase:?}, enabled={enabled}, locked={locked}"
                    );
                }
            }
        }
    }

    #[test]
    fn bubble_hides_during_break_even_when_enabled() {
        assert!(!bubble_visible(Phase::Breaking, true, false));
        assert!(bubble_visible(Phase::Paused, true, false));
        assert!(!bubble_visible(Phase::Working, true, true));
    }
}
