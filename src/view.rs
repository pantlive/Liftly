//! 仅包含可跨平台测试的展示逻辑，供 Windows 倒计时气泡使用。

use crate::schedule::Phase;

/// 倒计时气泡在 96 DPI 下的基准宽度。
pub const BUBBLE_BASE_WIDTH: i32 = 180;
/// 倒计时气泡在 96 DPI 下的基准高度。
pub const BUBBLE_BASE_HEIGHT: i32 = 42;
/// 气泡默认距工作区边缘的间距（96 DPI）。
pub const BUBBLE_BASE_MARGIN: i32 = 16;

/// 屏幕工作区，使用 Win32 的 `(left, top, right, bottom)` 约定，但不依赖 Windows 类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkArea {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl WorkArea {
    pub const fn new(left: i32, top: i32, right: i32, bottom: i32) -> Self {
        Self {
            left,
            top,
            right,
            bottom,
        }
    }

    pub const fn width(self) -> i32 {
        self.right.saturating_sub(self.left)
    }

    pub const fn height(self) -> i32 {
        self.bottom.saturating_sub(self.top)
    }
}

/// 气泡窗口的左上角与尺寸。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BubbleRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// 把 96 DPI 下的基准尺寸换算到目标 DPI；四舍五入，保证高 DPI 下文字不缩得过小。
pub fn scale_for_dpi(value: i32, dpi: u32) -> i32 {
    let dpi = if dpi == 0 {
        96
    } else {
        dpi.min(i32::MAX as u32) as i32
    };
    (value.saturating_mul(dpi).saturating_add(48)) / 96
}

pub fn bubble_size(dpi: u32) -> (i32, i32) {
    (
        scale_for_dpi(BUBBLE_BASE_WIDTH, dpi).max(1),
        scale_for_dpi(BUBBLE_BASE_HEIGHT, dpi).max(1),
    )
}

pub fn bubble_margin(dpi: u32) -> i32 {
    scale_for_dpi(BUBBLE_BASE_MARGIN, dpi).max(0)
}

/// 把气泡左上角夹到工作区内；工作区比气泡还小时优先保留左上角。
pub fn clamp_bubble_position(
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    work: WorkArea,
) -> (i32, i32) {
    if work.right <= work.left || work.bottom <= work.top {
        return (work.left, work.top);
    }
    let width = width.max(1);
    let height = height.max(1);
    let max_x = work.right.saturating_sub(width).max(work.left);
    let max_y = work.bottom.saturating_sub(height).max(work.top);
    (x.clamp(work.left, max_x), y.clamp(work.top, max_y))
}

/// 计算气泡位置：没有保存坐标时放在工作区右上角，有保存坐标时只做可见性夹取。
pub fn bubble_rect(dpi: u32, work: WorkArea, saved: Option<(i32, i32)>) -> BubbleRect {
    let (width, height) = bubble_size(dpi);
    let margin = bubble_margin(dpi);
    let (x, y) = saved.unwrap_or_else(|| {
        (
            work.right.saturating_sub(width).saturating_sub(margin),
            work.top.saturating_add(margin),
        )
    });
    let (x, y) = clamp_bubble_position(x, y, width, height, work);
    BubbleRect {
        x,
        y,
        width,
        height,
    }
}

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

    fn work(left: i32, top: i32, right: i32, bottom: i32) -> WorkArea {
        WorkArea::new(left, top, right, bottom)
    }

    #[test]
    fn bubble_size_and_margin_scale_by_dpi() {
        for (dpi, size, margin) in [
            (0, (180, 42), 16),
            (96, (180, 42), 16),
            (120, (225, 53), 20),
            (144, (270, 63), 24),
            (192, (360, 84), 32),
        ] {
            assert_eq!(bubble_size(dpi), size, "dpi={dpi}");
            assert_eq!(bubble_margin(dpi), margin, "dpi={dpi}");
        }
    }

    #[test]
    fn default_position_is_top_right_of_primary_work_area() {
        let area = work(0, 0, 1920, 1040);
        assert_eq!((area.width(), area.height()), (1920, 1040));
        let rect = bubble_rect(96, area, None);
        assert_eq!(
            rect,
            BubbleRect {
                x: 1724,
                y: 16,
                width: 180,
                height: 42,
            }
        );
        // 高 DPI 下按缩放后的尺寸和边距重新计算。
        let rect = bubble_rect(144, work(0, 0, 1920, 1040), None);
        assert_eq!(rect.x, 1920 - 270 - 24);
        assert_eq!(rect.y, 24);
    }

    #[test]
    fn saved_position_is_kept_when_it_is_visible() {
        let rect = bubble_rect(96, work(0, 0, 1920, 1040), Some((1200, 80)));
        assert_eq!((rect.x, rect.y), (1200, 80));
    }

    #[test]
    fn saved_position_is_clamped_to_work_area() {
        let area = work(0, 0, 1920, 1040);
        assert_eq!(
            bubble_rect(96, area, Some((5000, 5000))),
            BubbleRect {
                x: 1740,
                y: 998,
                width: 180,
                height: 42,
            }
        );
        assert_eq!(
            (
                bubble_rect(96, area, Some((-5000, -5000))).x,
                bubble_rect(96, area, Some((-5000, -5000))).y
            ),
            (0, 0)
        );
    }

    #[test]
    fn negative_secondary_monitor_coordinates_are_supported() {
        let area = work(-1920, 0, 0, 1040);
        // 副屏在主屏左侧时，默认位置和副屏上的坐标都保持负数。
        let default = bubble_rect(96, area, None);
        assert_eq!((default.x, default.y), (-196, 16));
        let saved = bubble_rect(96, area, Some((-1000, 100)));
        assert_eq!((saved.x, saved.y), (-1000, 100));
        let offscreen = bubble_rect(96, area, Some((-3000, -100)));
        assert_eq!((offscreen.x, offscreen.y), (-1920, 0));
    }

    #[test]
    fn tiny_work_area_keeps_origin_and_size() {
        let rect = bubble_rect(96, work(0, 0, 100, 40), None);
        assert_eq!((rect.x, rect.y), (0, 0));
        assert_eq!((rect.width, rect.height), (180, 42));
        let rect = bubble_rect(96, work(100, 100, 100, 100), Some((120, 130)));
        assert_eq!((rect.x, rect.y), (100, 100));
    }

    #[test]
    fn high_dpi_monitor_above_primary_keeps_negative_y() {
        let area = work(-1920, -1440, 640, 0);
        let saved = bubble_rect(192, area, Some((-800, -900)));
        assert_eq!((saved.x, saved.y), (-800, -900));
        assert_eq!((saved.width, saved.height), (360, 84));
        // 上方副屏边缘的坐标按缩放后的尺寸夹取，不能裁掉卡片。
        let edge = bubble_rect(192, area, Some((600, -10)));
        assert_eq!((edge.x, edge.y), (280, -84));
        let default = bubble_rect(192, area, None);
        assert_eq!((default.x, default.y), (248, -1408));
    }

    #[test]
    fn clamp_handles_work_area_offset_by_taskbar() {
        let area = work(100, 100, 1000, 700);
        assert_eq!(clamp_bubble_position(5000, 5000, 180, 42, area), (820, 658));
        assert_eq!(clamp_bubble_position(0, 0, 180, 42, area), (100, 100));
    }

    #[test]
    fn scaled_bubble_stays_within_secondary_work_area() {
        let area = work(-1920, -1080, 0, 0);
        for dpi in [96, 120, 144, 192] {
            // 对不同缩放下的默认位置和越界保存位置都检查完整可见性。
            for saved in [None, Some((-1, -1)), Some((-5000, -5000))] {
                let rect = bubble_rect(dpi, area, saved);
                assert!(rect.x >= area.left && rect.x + rect.width <= area.right);
                assert!(rect.y >= area.top && rect.y + rect.height <= area.bottom);
            }
            let tiny = bubble_rect(dpi, work(-100, -20, 0, 0), Some((10, 10)));
            assert_eq!((tiny.x, tiny.y), (-100, -20), "dpi={dpi}");
            assert_eq!((tiny.width, tiny.height), bubble_size(dpi));
        }
    }
}
