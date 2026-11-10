//! 仅包含可跨平台测试的展示逻辑，供 Windows 倒计时气泡使用。

use crate::config::{BubbleTheme, BUBBLE_SCALE_RANGE};
use crate::schedule::Phase;

/// 倒计时气泡在 96 DPI 下的基准宽度。压缩中间空白后仍能完整显示「距休息 600:00」。
pub const BUBBLE_BASE_WIDTH: i32 = 140;
/// 倒计时气泡在 96 DPI 下的基准高度。
pub const BUBBLE_BASE_HEIGHT: i32 = 42;
/// 气泡默认距工作区边缘的间距（96 DPI）。
pub const BUBBLE_BASE_MARGIN: i32 = 16;
/// 气泡左右内边距（96 DPI）。
pub const BUBBLE_PAD: i32 = 9;
/// 右侧品牌图标边长（96 DPI）。
pub const BUBBLE_ICON: i32 = 28;
/// 文字区与图标之间的间距（96 DPI）。
pub const BUBBLE_GAP: i32 = 7;

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

/// 把真实 DPI 和用户缩放百分比合成气泡绘制用的有效 DPI。
/// `dpi` 为 0 时按 96 处理；缩放百分比夹到合法范围；结果至少为 1。
pub fn bubble_dpi(dpi: u32, scale_percent: u32) -> u32 {
    let dpi = if dpi == 0 { 96 } else { dpi };
    let scale = scale_percent.clamp(*BUBBLE_SCALE_RANGE.start(), *BUBBLE_SCALE_RANGE.end());
    (dpi.saturating_mul(scale).saturating_add(50) / 100).max(1)
}

pub fn bubble_size(dpi: u32, scale: u32) -> (i32, i32) {
    let dpi = bubble_dpi(dpi, scale);
    (
        scale_for_dpi(BUBBLE_BASE_WIDTH, dpi).max(1),
        scale_for_dpi(BUBBLE_BASE_HEIGHT, dpi).max(1),
    )
}

pub fn bubble_margin(dpi: u32) -> i32 {
    scale_for_dpi(BUBBLE_BASE_MARGIN, dpi).max(0)
}

/// 当前窗口宽度下文字区的像素宽度；过窄时返回 0。
pub fn bubble_text_width(width: i32, dpi: u32) -> i32 {
    let pad = scale_for_dpi(BUBBLE_PAD, dpi);
    let icon = scale_for_dpi(BUBBLE_ICON, dpi);
    let gap = scale_for_dpi(BUBBLE_GAP, dpi);
    width
        .saturating_sub(pad)
        .saturating_sub(icon)
        .saturating_sub(gap)
        .saturating_sub(pad)
        .max(0)
}

/// 气泡配色，均为 RGB 三元组。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BubblePalette {
    pub background: (u8, u8, u8),
    pub border: (u8, u8, u8),
    pub countdown: (u8, u8, u8),
    pub status: (u8, u8, u8),
}

/// 按皮肤和工作/暂停状态返回配色。Green 工作态沿用原浅绿卡片。
pub fn bubble_palette(theme: BubbleTheme, paused: bool) -> BubblePalette {
    match (theme, paused) {
        (BubbleTheme::Green, false) => BubblePalette {
            background: (239, 248, 225),
            border: (115, 181, 83),
            countdown: (43, 82, 43),
            status: (58, 104, 48),
        },
        (BubbleTheme::Green, true) => BubblePalette {
            background: (255, 248, 225),
            border: (199, 162, 91),
            countdown: (43, 82, 43),
            status: (143, 99, 27),
        },
        (BubbleTheme::Blue, false) => BubblePalette {
            background: (225, 240, 255),
            border: (70, 140, 210),
            countdown: (30, 70, 120),
            status: (40, 90, 150),
        },
        (BubbleTheme::Blue, true) => BubblePalette {
            background: (232, 238, 246),
            border: (120, 140, 170),
            countdown: (50, 70, 100),
            status: (80, 90, 120),
        },
        (BubbleTheme::Orange, false) => BubblePalette {
            background: (255, 243, 224),
            border: (224, 140, 60),
            countdown: (140, 70, 20),
            status: (160, 85, 30),
        },
        (BubbleTheme::Orange, true) => BubblePalette {
            background: (255, 236, 210),
            border: (180, 110, 50),
            countdown: (120, 70, 30),
            status: (130, 80, 40),
        },
        (BubbleTheme::Dark, false) => BubblePalette {
            background: (40, 44, 52),
            border: (90, 160, 90),
            countdown: (210, 220, 210),
            status: (140, 190, 140),
        },
        (BubbleTheme::Dark, true) => BubblePalette {
            background: (50, 44, 36),
            border: (180, 140, 70),
            countdown: (230, 210, 170),
            status: (200, 170, 100),
        },
    }
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
/// 尺寸跟随缩放百分比，距边缘的 margin 仍按真实 DPI 计算。
pub fn bubble_rect(dpi: u32, scale: u32, work: WorkArea, saved: Option<(i32, i32)>) -> BubbleRect {
    let (width, height) = bubble_size(dpi, scale);
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

/// 拖动边缘调整大小时命中的边或角。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResizeEdge {
    Left,
    Right,
    Top,
    Bottom,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

impl ResizeEdge {
    const fn flips_dx(self) -> bool {
        matches!(self, Self::Left | Self::TopLeft | Self::BottomLeft)
    }

    const fn flips_dy(self) -> bool {
        matches!(self, Self::Top | Self::TopLeft | Self::TopRight)
    }
}

/// 气泡边缘可拖动调整大小的热区宽度（96 DPI 逻辑像素）。
pub const BUBBLE_RESIZE_BAND: i32 = 6;

/// 坐标落在离边 `band` 像素内时返回对应的边或角；中间区域返回 `None`。
/// 实际热区再夹到 `min(width, height) / 4`，保证缩小后中间仍能拖动移动。
pub fn resize_edge_at(x: i32, y: i32, width: i32, height: i32, band: i32) -> Option<ResizeEdge> {
    if width <= 0 || height <= 0 || x < 0 || y < 0 || x >= width || y >= height {
        return None;
    }
    let max_band = width.min(height) / 4;
    let band = band.min(max_band);
    if band <= 0 {
        return None;
    }
    let left = x < band;
    let right = x >= width.saturating_sub(band);
    let top = y < band;
    let bottom = y >= height.saturating_sub(band);
    match (left, right, top, bottom) {
        (true, false, true, false) => Some(ResizeEdge::TopLeft),
        (false, true, true, false) => Some(ResizeEdge::TopRight),
        (true, false, false, true) => Some(ResizeEdge::BottomLeft),
        (false, true, false, true) => Some(ResizeEdge::BottomRight),
        (true, false, false, false) => Some(ResizeEdge::Left),
        (false, true, false, false) => Some(ResizeEdge::Right),
        (false, false, true, false) => Some(ResizeEdge::Top),
        (false, false, false, true) => Some(ResizeEdge::Bottom),
        _ => None,
    }
}

/// 根据拖边位移计算新的缩放百分比，结果夹到 [`BUBBLE_SCALE_RANGE`]。
/// 左右边看宽度、上下边看高度；角取两个方向里相对起点百分比变化更大的轴。
/// `base_w` / `base_h` 是当前 DPI 下 100% 的尺寸。
#[allow(clippy::too_many_arguments)]
pub fn scale_from_resize(
    edge: ResizeEdge,
    start_w: i32,
    start_h: i32,
    dx: i32,
    dy: i32,
    base_w: i32,
    base_h: i32,
) -> u32 {
    let dx = if edge.flips_dx() {
        dx.saturating_neg()
    } else {
        dx
    };
    let dy = if edge.flips_dy() {
        dy.saturating_neg()
    } else {
        dy
    };
    let new_w = start_w.saturating_add(dx);
    let new_h = start_h.saturating_add(dy);
    let width_percent = percent_from_size(new_w, base_w);
    let height_percent = percent_from_size(new_h, base_h);
    match edge {
        ResizeEdge::Left | ResizeEdge::Right => width_percent,
        ResizeEdge::Top | ResizeEdge::Bottom => height_percent,
        _ if width_change_dominates(new_w, start_w, new_h, start_h) => width_percent,
        _ => height_percent,
    }
}

fn percent_from_size(size: i32, base: i32) -> u32 {
    let min = *BUBBLE_SCALE_RANGE.start();
    let max = *BUBBLE_SCALE_RANGE.end();
    if base <= 0 {
        return min;
    }
    let size = i64::from(size.max(0));
    let base = i64::from(base);
    let percent = size.saturating_mul(100).saturating_add(base / 2) / base;
    percent.clamp(i64::from(min), i64::from(max)) as u32
}

fn width_change_dominates(new_w: i32, start_w: i32, new_h: i32, start_h: i32) -> bool {
    let start_w = start_w.max(1);
    let start_h = start_h.max(1);
    let dw = (i64::from(new_w) - i64::from(start_w)).unsigned_abs();
    let dh = (i64::from(new_h) - i64::from(start_h)).unsigned_abs();
    dw.saturating_mul(start_h as u64) >= dh.saturating_mul(start_w as u64)
}

/// 保持对边或对角不动，返回缩放后的左上角。
pub fn resized_origin(edge: ResizeEdge, start: BubbleRect, new_w: i32, new_h: i32) -> (i32, i32) {
    let new_w = new_w.max(1);
    let new_h = new_h.max(1);
    let x = if edge.flips_dx() {
        start.x.saturating_add(start.width).saturating_sub(new_w)
    } else {
        start.x
    };
    let y = if edge.flips_dy() {
        start.y.saturating_add(start.height).saturating_sub(new_h)
    } else {
        start.y
    };
    (x, y)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::BUBBLE_THEME_PRESETS;

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
            (0, (140, 42), 16),
            (96, (140, 42), 16),
            (120, (175, 53), 20),
            (144, (210, 63), 24),
            (192, (280, 84), 32),
        ] {
            assert_eq!(bubble_size(dpi, 100), size, "dpi={dpi}");
            assert_eq!(bubble_margin(dpi), margin, "dpi={dpi}");
        }
    }

    #[test]
    fn default_position_is_top_right_of_primary_work_area() {
        let area = work(0, 0, 1920, 1040);
        assert_eq!((area.width(), area.height()), (1920, 1040));
        let rect = bubble_rect(96, 100, area, None);
        assert_eq!(
            rect,
            BubbleRect {
                x: 1764,
                y: 16,
                width: 140,
                height: 42,
            }
        );
        // 高 DPI 下按缩放后的尺寸和边距重新计算。
        let rect = bubble_rect(144, 100, work(0, 0, 1920, 1040), None);
        assert_eq!(rect.x, 1920 - 210 - 24);
        assert_eq!(rect.y, 24);
    }

    #[test]
    fn saved_position_is_kept_when_it_is_visible() {
        let rect = bubble_rect(96, 100, work(0, 0, 1920, 1040), Some((1200, 80)));
        assert_eq!((rect.x, rect.y), (1200, 80));
    }

    #[test]
    fn saved_position_is_clamped_to_work_area() {
        let area = work(0, 0, 1920, 1040);
        assert_eq!(
            bubble_rect(96, 100, area, Some((5000, 5000))),
            BubbleRect {
                x: 1780,
                y: 998,
                width: 140,
                height: 42,
            }
        );
        assert_eq!(
            (
                bubble_rect(96, 100, area, Some((-5000, -5000))).x,
                bubble_rect(96, 100, area, Some((-5000, -5000))).y
            ),
            (0, 0)
        );
    }

    #[test]
    fn negative_secondary_monitor_coordinates_are_supported() {
        let area = work(-1920, 0, 0, 1040);
        // 副屏在主屏左侧时，默认位置和副屏上的坐标都保持负数。
        let default = bubble_rect(96, 100, area, None);
        assert_eq!((default.x, default.y), (-156, 16));
        let saved = bubble_rect(96, 100, area, Some((-1000, 100)));
        assert_eq!((saved.x, saved.y), (-1000, 100));
        let offscreen = bubble_rect(96, 100, area, Some((-3000, -100)));
        assert_eq!((offscreen.x, offscreen.y), (-1920, 0));
    }

    #[test]
    fn tiny_work_area_keeps_origin_and_size() {
        let rect = bubble_rect(96, 100, work(0, 0, 100, 40), None);
        assert_eq!((rect.x, rect.y), (0, 0));
        assert_eq!((rect.width, rect.height), (140, 42));
        let rect = bubble_rect(96, 100, work(100, 100, 100, 100), Some((120, 130)));
        assert_eq!((rect.x, rect.y), (100, 100));
    }

    #[test]
    fn high_dpi_monitor_above_primary_keeps_negative_y() {
        let area = work(-1920, -1440, 640, 0);
        let saved = bubble_rect(192, 100, area, Some((-800, -900)));
        assert_eq!((saved.x, saved.y), (-800, -900));
        assert_eq!((saved.width, saved.height), (280, 84));
        // 上方副屏边缘的坐标按缩放后的尺寸夹取，不能裁掉卡片。
        let edge = bubble_rect(192, 100, area, Some((600, -10)));
        assert_eq!((edge.x, edge.y), (360, -84));
        let default = bubble_rect(192, 100, area, None);
        assert_eq!((default.x, default.y), (328, -1408));
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
                let rect = bubble_rect(dpi, 100, area, saved);
                assert!(rect.x >= area.left && rect.x + rect.width <= area.right);
                assert!(rect.y >= area.top && rect.y + rect.height <= area.bottom);
            }
            let tiny = bubble_rect(dpi, 100, work(-100, -20, 0, 0), Some((10, 10)));
            assert_eq!((tiny.x, tiny.y), (-100, -20), "dpi={dpi}");
            assert_eq!((tiny.width, tiny.height), bubble_size(dpi, 100));
        }
    }

    #[test]
    fn bubble_scale_100_matches_base_size() {
        assert_eq!(bubble_size(96, 100), (140, 42));
        assert_eq!(bubble_size(0, 100), bubble_size(96, 100));
        assert_eq!(bubble_dpi(0, 100), 96);
        assert_eq!(bubble_dpi(96, 100), 96);
    }

    #[test]
    fn bubble_scale_resizes_at_96_dpi() {
        assert_eq!(bubble_size(96, 150), (210, 63));
        assert_eq!(bubble_size(96, 50), (70, 21));
    }

    #[test]
    fn bubble_scale_out_of_range_is_clamped() {
        assert_eq!(bubble_dpi(96, 49), bubble_dpi(96, 50));
        assert_eq!(bubble_dpi(96, 301), bubble_dpi(96, 300));
        assert_eq!(bubble_size(96, 0), bubble_size(96, 50));
        assert_eq!(bubble_size(96, 999), bubble_size(96, 300));
    }

    #[test]
    fn scaled_auto_position_stays_top_right_with_unscaled_margin() {
        let area = work(0, 0, 1920, 1040);
        let rect = bubble_rect(96, 150, area, None);
        assert_eq!(
            rect,
            BubbleRect {
                x: 1920 - 210 - 16,
                y: 16,
                width: 210,
                height: 63,
            }
        );
        assert!(rect.x >= area.left && rect.x + rect.width <= area.right);
        assert!(rect.y >= area.top && rect.y + rect.height <= area.bottom);
    }

    #[test]
    fn scaled_bubble_rect_stays_within_work_area() {
        let area = work(-1920, -1080, 0, 0);
        for scale in [50, 80, 100, 125, 150, 200, 300, 999] {
            for saved in [None, Some((-1, -1)), Some((-5000, -5000))] {
                let rect = bubble_rect(96, scale, area, saved);
                assert!(
                    rect.x >= area.left && rect.x + rect.width <= area.right,
                    "scale={scale} saved={saved:?} rect={rect:?}"
                );
                assert!(
                    rect.y >= area.top && rect.y + rect.height <= area.bottom,
                    "scale={scale} saved={saved:?} rect={rect:?}"
                );
            }
        }
        let saved = bubble_rect(96, 200, work(0, 0, 1920, 1040), Some((1800, 10)));
        assert_eq!((saved.width, saved.height), (280, 84));
        assert_eq!((saved.x, saved.y), (1640, 10));
    }

    #[test]
    fn resize_edge_hits_all_eight_directions_and_ignores_center() {
        let (w, h, band) = (180, 42, BUBBLE_RESIZE_BAND);
        assert_eq!(resize_edge_at(0, 21, w, h, band), Some(ResizeEdge::Left));
        assert_eq!(resize_edge_at(5, 21, w, h, band), Some(ResizeEdge::Left));
        assert_eq!(resize_edge_at(6, 21, w, h, band), None);
        assert_eq!(resize_edge_at(179, 21, w, h, band), Some(ResizeEdge::Right));
        assert_eq!(resize_edge_at(174, 21, w, h, band), Some(ResizeEdge::Right));
        assert_eq!(resize_edge_at(173, 21, w, h, band), None);
        assert_eq!(resize_edge_at(90, 0, w, h, band), Some(ResizeEdge::Top));
        assert_eq!(resize_edge_at(90, 5, w, h, band), Some(ResizeEdge::Top));
        assert_eq!(resize_edge_at(90, 6, w, h, band), None);
        assert_eq!(resize_edge_at(90, 41, w, h, band), Some(ResizeEdge::Bottom));
        assert_eq!(resize_edge_at(90, 36, w, h, band), Some(ResizeEdge::Bottom));
        assert_eq!(resize_edge_at(90, 35, w, h, band), None);
        assert_eq!(resize_edge_at(0, 0, w, h, band), Some(ResizeEdge::TopLeft));
        assert_eq!(
            resize_edge_at(179, 0, w, h, band),
            Some(ResizeEdge::TopRight)
        );
        assert_eq!(
            resize_edge_at(0, 41, w, h, band),
            Some(ResizeEdge::BottomLeft)
        );
        assert_eq!(
            resize_edge_at(179, 41, w, h, band),
            Some(ResizeEdge::BottomRight)
        );
        assert_eq!(resize_edge_at(90, 21, w, h, band), None);
        assert_eq!(resize_edge_at(-1, 0, w, h, band), None);
        assert_eq!(resize_edge_at(180, 21, w, h, band), None);
    }

    #[test]
    fn resize_band_is_clamped_on_small_bubbles() {
        // 50% 缩放约 90×21；band=6 应夹到 min/4=5，中间仍可拖动移动。
        assert_eq!(resize_edge_at(4, 10, 90, 21, 6), Some(ResizeEdge::Left));
        assert_eq!(resize_edge_at(5, 10, 90, 21, 6), None);
        assert_eq!(resize_edge_at(45, 10, 90, 21, 6), None);
        assert_eq!(resize_edge_at(85, 10, 90, 21, 6), Some(ResizeEdge::Right));
        assert_eq!(resize_edge_at(84, 10, 90, 21, 6), None);
        assert_eq!(resize_edge_at(10, 10, 20, 20, 6), None);
        assert_eq!(resize_edge_at(4, 10, 20, 20, 6), Some(ResizeEdge::Left));
        assert_eq!(resize_edge_at(5, 10, 20, 20, 6), None);
    }

    #[test]
    fn scale_from_resize_grows_from_right_and_left() {
        assert_eq!(
            scale_from_resize(ResizeEdge::Right, 180, 42, 90, 0, 180, 42),
            150
        );
        assert_eq!(
            scale_from_resize(ResizeEdge::Left, 180, 42, -90, 0, 180, 42),
            150
        );
        assert_eq!(
            scale_from_resize(ResizeEdge::Bottom, 180, 42, 0, 21, 180, 42),
            150
        );
        assert_eq!(
            scale_from_resize(ResizeEdge::Top, 180, 42, 0, -21, 180, 42),
            150
        );
    }

    #[test]
    fn scale_from_resize_clamps_to_legal_range() {
        assert_eq!(
            scale_from_resize(ResizeEdge::Right, 180, 42, 10_000, 0, 180, 42),
            300
        );
        assert_eq!(
            scale_from_resize(ResizeEdge::Right, 180, 42, -180, 0, 180, 42),
            50
        );
        assert_eq!(
            scale_from_resize(ResizeEdge::Left, 180, 42, 180, 0, 180, 42),
            50
        );
        assert_eq!(
            scale_from_resize(ResizeEdge::Left, 180, 42, -10_000, 0, 180, 42),
            300
        );
        assert_eq!(
            scale_from_resize(ResizeEdge::Bottom, 180, 42, 0, 10_000, 180, 42),
            300
        );
        assert_eq!(
            scale_from_resize(ResizeEdge::Top, 180, 42, 0, 42, 180, 42),
            50
        );
    }

    #[test]
    fn scale_from_resize_corner_picks_larger_axis() {
        // 宽度 +50%、高度约 +10% → 取宽度。
        assert_eq!(
            scale_from_resize(ResizeEdge::BottomRight, 180, 42, 90, 4, 180, 42),
            150
        );
        // 高度 +50%、宽度 +10% → 取高度。
        assert_eq!(
            scale_from_resize(ResizeEdge::BottomRight, 180, 42, 18, 21, 180, 42),
            150
        );
        // 左上角往外拉，dx/dy 取反后同样放大。
        assert_eq!(
            scale_from_resize(ResizeEdge::TopLeft, 180, 42, -90, -4, 180, 42),
            150
        );
        assert_eq!(
            scale_from_resize(ResizeEdge::TopLeft, 180, 42, -18, -21, 180, 42),
            150
        );
    }

    #[test]
    fn resized_origin_keeps_opposite_edge_fixed() {
        let start = BubbleRect {
            x: 100,
            y: 50,
            width: 180,
            height: 42,
        };
        let (x, y) = resized_origin(ResizeEdge::Left, start, 270, 63);
        assert_eq!((x, y), (10, 50));
        assert_eq!(x + 270, start.x + start.width);
        assert_eq!(y, start.y);

        let (x, y) = resized_origin(ResizeEdge::Top, start, 270, 63);
        assert_eq!((x, y), (100, 29));
        assert_eq!(x, start.x);
        assert_eq!(y + 63, start.y + start.height);

        assert_eq!(resized_origin(ResizeEdge::Right, start, 270, 63), (100, 50));
        assert_eq!(
            resized_origin(ResizeEdge::Bottom, start, 270, 63),
            (100, 50)
        );
        assert_eq!(
            resized_origin(ResizeEdge::TopLeft, start, 270, 63),
            (10, 29)
        );
        assert_eq!(
            resized_origin(ResizeEdge::BottomRight, start, 270, 63),
            (100, 50)
        );

        let secondary = BubbleRect {
            x: -200,
            y: -100,
            width: 180,
            height: 42,
        };
        assert_eq!(
            resized_origin(ResizeEdge::Left, secondary, 270, 63),
            (-290, -100)
        );
        assert_eq!(
            resized_origin(ResizeEdge::Top, secondary, 270, 63),
            (-200, -121)
        );
    }

    #[test]
    fn bubble_text_width_fits_longest_clock_at_100_percent() {
        let (width, _) = bubble_size(96, 100);
        assert_eq!(width, 140);
        let text = bubble_text_width(width, 96);
        // 「距休息 600:00」约 83px；96 DPI 标准宽度下文字区为 87px。
        assert!(text >= 87, "text_width={text}");
    }

    #[test]
    fn bubble_text_width_stays_positive_across_scale_range() {
        for scale in *BUBBLE_SCALE_RANGE.start()..=*BUBBLE_SCALE_RANGE.end() {
            let (width, _) = bubble_size(96, scale);
            let dpi = bubble_dpi(96, scale);
            let text = bubble_text_width(width, dpi);
            assert!(
                text > 0,
                "scale={scale} width={width} dpi={dpi} text={text}"
            );
        }
    }

    #[test]
    fn palettes_avoid_color_key_and_green_matches_original() {
        const KEY: (u8, u8, u8) = (255, 0, 255);
        for theme in BUBBLE_THEME_PRESETS {
            for paused in [false, true] {
                let palette = bubble_palette(theme, paused);
                for color in [
                    palette.background,
                    palette.border,
                    palette.countdown,
                    palette.status,
                ] {
                    assert_ne!(
                        color, KEY,
                        "theme={theme:?} paused={paused} color={color:?}"
                    );
                }
            }
        }
        let working = bubble_palette(BubbleTheme::Green, false);
        assert_eq!(working.background, (239, 248, 225));
        assert_eq!(working.border, (115, 181, 83));
        assert_eq!(working.countdown, (43, 82, 43));
        assert_eq!(working.status, (58, 104, 48));
        let paused = bubble_palette(BubbleTheme::Green, true);
        assert_eq!(paused.background, (255, 248, 225));
        assert_eq!(paused.border, (199, 162, 91));
        assert_eq!(paused.countdown, (43, 82, 43));
        assert_eq!(paused.status, (143, 99, 27));
    }
}
