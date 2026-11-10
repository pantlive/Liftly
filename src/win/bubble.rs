//! 桌面倒计时气泡。单行圆角卡片，可拖动、拖边缩放、切换皮肤并按显示器工作区夹取。

use std::sync::atomic::{AtomicBool, AtomicI32, AtomicIsize, AtomicU32, AtomicU64, Ordering};
use std::sync::Once;

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    BeginPaint, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, CreateFontW,
    CreateRoundRectRgn, CreateSolidBrush, DeleteDC, DeleteObject, DrawTextW, EndPaint, FillRect,
    FillRgn, FrameRgn, GetMonitorInfoW, InvalidateRect, MonitorFromRect, MonitorFromWindow,
    SelectObject, SetBkMode, SetTextColor, CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET,
    DEFAULT_PITCH, DT_LEFT, DT_NOPREFIX, DT_SINGLELINE, DT_VCENTER, FF_DONTCARE, FW_NORMAL, HDC,
    HMONITOR, MONITORINFO, MONITOR_DEFAULTTONEAREST, OUT_DEFAULT_PRECIS, PAINTSTRUCT, SRCCOPY,
    TRANSPARENT,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::HiDpi::{GetDpiForSystem, GetDpiForWindow};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetCapture, ReleaseCapture, SetCapture};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DrawIconEx, GetClientRect, GetCursorPos,
    GetSystemMetrics, GetWindowRect, IsWindow, LoadCursorW, PostMessageW, RegisterClassW,
    SetCursor, SetLayeredWindowAttributes, SetWindowPos, ShowWindow, SystemParametersInfoW,
    DI_NORMAL, HTCLIENT, HTTRANSPARENT, HWND_TOPMOST, IDC_SIZEALL, IDC_SIZENESW, IDC_SIZENS,
    IDC_SIZENWSE, IDC_SIZEWE, LWA_ALPHA, LWA_COLORKEY, MA_NOACTIVATE, SM_CXSCREEN, SM_CYSCREEN,
    SPI_GETWORKAREA, SWP_NOACTIVATE, SWP_NOSIZE, SW_SHOWNOACTIVATE, WM_CAPTURECHANGED, WM_CLOSE,
    WM_DESTROY, WM_DPICHANGED, WM_ERASEBKGND, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEACTIVATE,
    WM_MOUSEMOVE, WM_NCHITTEST, WM_PAINT, WM_SETCURSOR, WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};

use super::{wide, AppIcon};
use crate::config::BubbleTheme;
use crate::view::{self, BubbleRect, ResizeEdge, WorkArea};

const BUBBLE_CLASS: &str = "Liftly.CountdownBubble";
const BASE_RADIUS: i32 = 6;
const ALPHA: u8 = 248;
const DRAG_THRESHOLD: i32 = 3;

static CLASS_REGISTERED: Once = Once::new();
static BUBBLE_HWND: AtomicIsize = AtomicIsize::new(0);
static MAIN_HWND: AtomicIsize = AtomicIsize::new(0);
static REMAINING_MS: AtomicU64 = AtomicU64::new(0);
static PAUSED: AtomicBool = AtomicBool::new(false);
static DRAG_ACTIVE: AtomicBool = AtomicBool::new(false);
static DRAG_STARTED: AtomicBool = AtomicBool::new(false);
static DRAG_CURSOR_X: AtomicI32 = AtomicI32::new(0);
static DRAG_CURSOR_Y: AtomicI32 = AtomicI32::new(0);
static DRAG_WINDOW_X: AtomicI32 = AtomicI32::new(0);
static DRAG_WINDOW_Y: AtomicI32 = AtomicI32::new(0);
static BUBBLE_SCALE: AtomicU32 = AtomicU32::new(100);
/// 当前皮肤的编码，见 [`theme_code`]。
static BUBBLE_THEME: AtomicU32 = AtomicU32::new(0);
/// 0 表示没有在调整大小；非 0 为 [`ResizeEdge`] 的编码。
static RESIZE_EDGE: AtomicU32 = AtomicU32::new(0);
static RESIZE_START_W: AtomicI32 = AtomicI32::new(0);
static RESIZE_START_H: AtomicI32 = AtomicI32::new(0);
static RESIZE_START_SCALE: AtomicU32 = AtomicU32::new(100);
/// 鼠标缩放已改窗口尺寸，但主窗口尚未把 `bubble_scale` 写回配置。
static SCALE_PENDING: AtomicBool = AtomicBool::new(false);

/// 倒计时气泡。窗口句柄统一保存在静态变量中，避免结构体继续操作已销毁的句柄。
#[derive(Default)]
pub struct Bubble {
    failed: bool,
}

impl Bubble {
    pub fn new(owner: HWND) -> Self {
        MAIN_HWND.store(owner as isize, Ordering::SeqCst);
        Self::default()
    }

    /// 更新显示内容；窗口不存在时按保存位置创建，之后仅重绘，避免打断拖动。
    pub fn show(
        &mut self,
        remaining_ms: u64,
        paused: bool,
        saved: Option<(i32, i32)>,
        scale: u32,
        theme: BubbleTheme,
    ) -> bool {
        REMAINING_MS.store(remaining_ms, Ordering::SeqCst);
        PAUSED.store(paused, Ordering::SeqCst);
        BUBBLE_THEME.store(theme_code(theme), Ordering::SeqCst);

        let mut hwnd = current_hwnd();
        if hwnd.is_null() {
            BUBBLE_SCALE.store(scale, Ordering::SeqCst);
            ensure_class();
            hwnd = unsafe { create_bubble_window(saved) };
            if hwnd.is_null() {
                self.failed = true;
                return false;
            }
            BUBBLE_HWND.store(hwnd as isize, Ordering::SeqCst);
            unsafe {
                // 颜色键让圆角外的区域完全透明，整体 alpha 再提供半透明卡片效果。
                if SetLayeredWindowAttributes(hwnd, key_color(), ALPHA, LWA_ALPHA | LWA_COLORKEY)
                    == 0
                {
                    // 极旧系统不支持组合标志时至少保留圆角透明，卡片改为不透明。
                    SetLayeredWindowAttributes(hwnd, key_color(), 0, LWA_COLORKEY);
                }
                // 创建时按系统 DPI 估算；再按窗口实际 DPI 和用户缩放校正尺寸、边距和保存位置。
                let rect = view::bubble_rect(
                    dpi_for_window(hwnd),
                    scale,
                    work_area_for_window(hwnd),
                    saved,
                );
                SetWindowPos(
                    hwnd,
                    HWND_TOPMOST,
                    rect.x,
                    rect.y,
                    rect.width,
                    rect.height,
                    SWP_NOACTIVATE,
                );
                if saved.is_some_and(|position| position != (rect.x, rect.y)) {
                    notify_moved(hwnd);
                }
                ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            }
            self.failed = false;
        } else if BUBBLE_SCALE.load(Ordering::SeqCst) != scale
            && !DRAG_ACTIVE.load(Ordering::SeqCst)
            && !SCALE_PENDING.load(Ordering::SeqCst)
        {
            BUBBLE_SCALE.store(scale, Ordering::SeqCst);
            unsafe {
                apply_new_scale(hwnd, saved);
            }
        }
        if BUBBLE_SCALE.load(Ordering::SeqCst) == scale {
            SCALE_PENDING.store(false, Ordering::SeqCst);
        }

        unsafe {
            InvalidateRect(hwnd, std::ptr::null(), 0);
        }
        true
    }

    /// 显示设置变化后按当前 DPI 重算尺寸，并把窗口夹回最近的显示器工作区。
    pub fn reclamp(&self) {
        let hwnd = current_hwnd();
        if hwnd.is_null() {
            return;
        }
        unsafe {
            if clamp_window(hwnd) {
                notify_moved(hwnd);
            }
            InvalidateRect(hwnd, std::ptr::null(), 0);
        }
    }

    /// 关闭气泡；可重复调用。
    pub fn hide(&mut self) {
        let hwnd = current_hwnd();
        if !hwnd.is_null() {
            unsafe {
                // DestroyWindow 会释放捕获，但显式释放可避免拖动状态短暂残留。
                cancel_drag(hwnd);
                DestroyWindow(hwnd);
            }
            let _ =
                BUBBLE_HWND.compare_exchange(hwnd as isize, 0, Ordering::SeqCst, Ordering::SeqCst);
        }
        REMAINING_MS.store(0, Ordering::SeqCst);
        SCALE_PENDING.store(false, Ordering::SeqCst);
    }

    pub fn reset_failure(&mut self) {
        self.failed = false;
    }

    pub fn failed(&self) -> bool {
        self.failed
    }
}

fn current_hwnd() -> HWND {
    let raw = BUBBLE_HWND.load(Ordering::SeqCst);
    if raw == 0 {
        return std::ptr::null_mut();
    }
    let hwnd = raw as HWND;
    if unsafe { IsWindow(hwnd) } == 0 {
        let _ = BUBBLE_HWND.compare_exchange(raw, 0, Ordering::SeqCst, Ordering::SeqCst);
        return std::ptr::null_mut();
    }
    hwnd
}

fn ensure_class() {
    CLASS_REGISTERED.call_once(|| unsafe {
        let hinstance = GetModuleHandleW(std::ptr::null());
        let class = wide(BUBBLE_CLASS);
        let wc = WNDCLASSW {
            style: 0,
            lpfnWndProc: Some(bubble_wnd_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: hinstance,
            hIcon: std::ptr::null_mut(),
            // 中间区域的类光标提示可以移动；边缘光标由 WM_SETCURSOR 覆盖。
            hCursor: LoadCursorW(std::ptr::null_mut(), IDC_SIZEALL),
            hbrBackground: std::ptr::null_mut(),
            lpszMenuName: std::ptr::null(),
            lpszClassName: class.as_ptr(),
        };
        RegisterClassW(&wc);
    });
}

unsafe fn create_bubble_window(saved: Option<(i32, i32)>) -> HWND {
    let dpi = system_dpi();
    let scale = BUBBLE_SCALE.load(Ordering::SeqCst);
    // 有保存坐标时在坐标所在的显示器上夹取，支持副屏负坐标；否则使用主屏工作区。
    let area = match saved {
        Some(point) => work_area_for_point(point),
        None => primary_work_area(),
    };
    let rect = view::bubble_rect(dpi, scale, area, saved);

    let hinstance = GetModuleHandleW(std::ptr::null());
    let class = wide(BUBBLE_CLASS);
    let title = wide(crate::i18n::bubble_window_title(crate::i18n::current()));
    CreateWindowExW(
        WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_LAYERED,
        class.as_ptr(),
        title.as_ptr(),
        WS_POPUP,
        rect.x,
        rect.y,
        rect.width,
        rect.height,
        std::ptr::null_mut(),
        std::ptr::null_mut(),
        hinstance,
        std::ptr::null(),
    )
}

unsafe extern "system" fn bubble_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_PAINT => {
            paint(hwnd);
            0
        }
        WM_ERASEBKGND => 1,
        // 卡片区域接收鼠标用于拖动，圆角外继续穿透到桌面。
        WM_NCHITTEST => hit_test(hwnd, lparam),
        WM_MOUSEACTIVATE => MA_NOACTIVATE as LRESULT,
        WM_SETCURSOR => apply_hover_cursor(hwnd, wparam, lparam),
        WM_LBUTTONDOWN => {
            if let Some(edge) = resize_edge_at_cursor(hwnd) {
                begin_resize(hwnd, edge);
            } else {
                begin_drag(hwnd);
            }
            0
        }
        WM_MOUSEMOVE => {
            if RESIZE_EDGE.load(Ordering::SeqCst) != 0 {
                resize_to_cursor(hwnd);
            } else {
                drag_to_cursor(hwnd);
            }
            0
        }
        WM_LBUTTONUP => {
            finish_drag(hwnd, true);
            0
        }
        WM_CAPTURECHANGED => {
            finish_drag(hwnd, false);
            0
        }
        WM_DPICHANGED => {
            // 未采用系统建议矩形，而是按新 DPI 重新计算紧凑尺寸并夹到工作区。
            if clamp_window(hwnd) {
                notify_moved(hwnd);
            }
            InvalidateRect(hwnd, std::ptr::null(), 0);
            0
        }
        WM_CLOSE => {
            DestroyWindow(hwnd);
            0
        }
        WM_DESTROY => {
            reset_drag_state();
            if BUBBLE_HWND.load(Ordering::SeqCst) == hwnd as isize {
                BUBBLE_HWND.store(0, Ordering::SeqCst);
            }
            0
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

/// 只让圆角卡片本身参与命中测试，避免透明角落挡住桌面鼠标操作。
unsafe fn hit_test(hwnd: HWND, lparam: LPARAM) -> LRESULT {
    let mut rect: RECT = std::mem::zeroed();
    if GetWindowRect(hwnd, &mut rect) == 0 {
        return HTTRANSPARENT as LRESULT;
    }
    let (screen_x, screen_y) = point_from_lparam(lparam);
    let x = screen_x.saturating_sub(rect.left);
    let y = screen_y.saturating_sub(rect.top);
    let width = rect.right.saturating_sub(rect.left);
    let height = rect.bottom.saturating_sub(rect.top);
    if x < 0 || y < 0 || x >= width || y >= height {
        return HTTRANSPARENT as LRESULT;
    }
    let radius = radius_for(height);
    if outside_rounded_corners(x, y, width, height, radius) {
        HTTRANSPARENT as LRESULT
    } else {
        HTCLIENT as LRESULT
    }
}

fn point_from_lparam(lparam: LPARAM) -> (i32, i32) {
    let packed = lparam as u32;
    (
        (packed & 0xffff) as u16 as i16 as i32,
        ((packed >> 16) & 0xffff) as u16 as i16 as i32,
    )
}

fn outside_rounded_corners(x: i32, y: i32, width: i32, height: i32, radius: i32) -> bool {
    if radius <= 0 || width <= 0 || height <= 0 {
        return false;
    }
    let radius_squared = radius.saturating_mul(radius);
    let right = width.saturating_sub(radius);
    let bottom = height.saturating_sub(radius);
    let corners = [
        (x < radius && y < radius, radius, radius),
        (x >= right && y < radius, right, radius),
        (x < radius && y >= bottom, radius, bottom),
        (x >= right && y >= bottom, right, bottom),
    ];
    corners.into_iter().any(|(in_corner, center_x, center_y)| {
        if !in_corner {
            return false;
        }
        let dx = x.saturating_sub(center_x);
        let dy = y.saturating_sub(center_y);
        dx.saturating_mul(dx).saturating_add(dy.saturating_mul(dy)) > radius_squared
    })
}

unsafe fn apply_hover_cursor(hwnd: HWND, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let hit = (lparam as u32) & 0xffff;
    if hit != HTCLIENT {
        return DefWindowProcW(hwnd, WM_SETCURSOR, wparam, lparam);
    }
    let edge =
        edge_from_code(RESIZE_EDGE.load(Ordering::SeqCst)).or_else(|| resize_edge_at_cursor(hwnd));
    if let Some(edge) = edge {
        set_resize_cursor(edge);
        1
    } else {
        DefWindowProcW(hwnd, WM_SETCURSOR, wparam, lparam)
    }
}

unsafe fn resize_edge_at_cursor(hwnd: HWND) -> Option<ResizeEdge> {
    let mut point: POINT = std::mem::zeroed();
    let mut rect: RECT = std::mem::zeroed();
    if GetCursorPos(&mut point) == 0 || GetWindowRect(hwnd, &mut rect) == 0 {
        return None;
    }
    let width = rect.right.saturating_sub(rect.left);
    let height = rect.bottom.saturating_sub(rect.top);
    let x = point.x.saturating_sub(rect.left);
    let y = point.y.saturating_sub(rect.top);
    let band = view::scale_for_dpi(view::BUBBLE_RESIZE_BAND, dpi_for_window(hwnd)).max(1);
    view::resize_edge_at(x, y, width, height, band)
}

fn set_resize_cursor(edge: ResizeEdge) {
    let idc = match edge {
        ResizeEdge::Left | ResizeEdge::Right => IDC_SIZEWE,
        ResizeEdge::Top | ResizeEdge::Bottom => IDC_SIZENS,
        ResizeEdge::TopLeft | ResizeEdge::BottomRight => IDC_SIZENWSE,
        ResizeEdge::TopRight | ResizeEdge::BottomLeft => IDC_SIZENESW,
    };
    unsafe {
        SetCursor(LoadCursorW(std::ptr::null_mut(), idc));
    }
}

unsafe fn begin_resize(hwnd: HWND, edge: ResizeEdge) {
    let mut point: POINT = std::mem::zeroed();
    let mut rect: RECT = std::mem::zeroed();
    if GetCursorPos(&mut point) == 0 || GetWindowRect(hwnd, &mut rect) == 0 {
        return;
    }
    DRAG_CURSOR_X.store(point.x, Ordering::SeqCst);
    DRAG_CURSOR_Y.store(point.y, Ordering::SeqCst);
    DRAG_WINDOW_X.store(rect.left, Ordering::SeqCst);
    DRAG_WINDOW_Y.store(rect.top, Ordering::SeqCst);
    RESIZE_START_W.store(rect.right.saturating_sub(rect.left), Ordering::SeqCst);
    RESIZE_START_H.store(rect.bottom.saturating_sub(rect.top), Ordering::SeqCst);
    RESIZE_START_SCALE.store(BUBBLE_SCALE.load(Ordering::SeqCst), Ordering::SeqCst);
    RESIZE_EDGE.store(edge_code(edge), Ordering::SeqCst);
    DRAG_STARTED.store(false, Ordering::SeqCst);
    DRAG_ACTIVE.store(true, Ordering::SeqCst);
    set_resize_cursor(edge);
    SetCapture(hwnd);
}

unsafe fn begin_drag(hwnd: HWND) {
    let mut point: POINT = std::mem::zeroed();
    let mut rect: RECT = std::mem::zeroed();
    if GetCursorPos(&mut point) == 0 || GetWindowRect(hwnd, &mut rect) == 0 {
        return;
    }
    DRAG_CURSOR_X.store(point.x, Ordering::SeqCst);
    DRAG_CURSOR_Y.store(point.y, Ordering::SeqCst);
    DRAG_WINDOW_X.store(rect.left, Ordering::SeqCst);
    DRAG_WINDOW_Y.store(rect.top, Ordering::SeqCst);
    RESIZE_EDGE.store(0, Ordering::SeqCst);
    DRAG_STARTED.store(false, Ordering::SeqCst);
    DRAG_ACTIVE.store(true, Ordering::SeqCst);
    SetCapture(hwnd);
}

unsafe fn drag_to_cursor(hwnd: HWND) {
    if !DRAG_ACTIVE.load(Ordering::SeqCst) || GetCapture() != hwnd {
        return;
    }
    let mut point: POINT = std::mem::zeroed();
    if GetCursorPos(&mut point) == 0 {
        return;
    }
    let dx = point.x.saturating_sub(DRAG_CURSOR_X.load(Ordering::SeqCst));
    let dy = point.y.saturating_sub(DRAG_CURSOR_Y.load(Ordering::SeqCst));
    if !DRAG_STARTED.load(Ordering::SeqCst) {
        if dx.saturating_abs().max(dy.saturating_abs()) < DRAG_THRESHOLD {
            return;
        }
        DRAG_STARTED.store(true, Ordering::SeqCst);
    }
    let x = DRAG_WINDOW_X.load(Ordering::SeqCst).saturating_add(dx);
    let y = DRAG_WINDOW_Y.load(Ordering::SeqCst).saturating_add(dy);
    SetWindowPos(hwnd, HWND_TOPMOST, x, y, 0, 0, SWP_NOSIZE | SWP_NOACTIVATE);
}

unsafe fn resize_to_cursor(hwnd: HWND) {
    let Some(edge) = edge_from_code(RESIZE_EDGE.load(Ordering::SeqCst)) else {
        return;
    };
    if !DRAG_ACTIVE.load(Ordering::SeqCst) || GetCapture() != hwnd {
        return;
    }
    let mut point: POINT = std::mem::zeroed();
    if GetCursorPos(&mut point) == 0 {
        return;
    }
    let dx = point.x.saturating_sub(DRAG_CURSOR_X.load(Ordering::SeqCst));
    let dy = point.y.saturating_sub(DRAG_CURSOR_Y.load(Ordering::SeqCst));
    if !DRAG_STARTED.load(Ordering::SeqCst) {
        if dx.saturating_abs().max(dy.saturating_abs()) < DRAG_THRESHOLD {
            return;
        }
        DRAG_STARTED.store(true, Ordering::SeqCst);
    }
    set_resize_cursor(edge);
    let start_w = RESIZE_START_W.load(Ordering::SeqCst);
    let start_h = RESIZE_START_H.load(Ordering::SeqCst);
    let dpi = dpi_for_window(hwnd);
    let (base_w, base_h) = view::bubble_size(dpi, 100);
    let scale = view::scale_from_resize(edge, start_w, start_h, dx, dy, base_w, base_h);
    if scale == BUBBLE_SCALE.load(Ordering::SeqCst) {
        return;
    }
    BUBBLE_SCALE.store(scale, Ordering::SeqCst);
    SCALE_PENDING.store(true, Ordering::SeqCst);
    let (width, height) = view::bubble_size(dpi, scale);
    let start = BubbleRect {
        x: DRAG_WINDOW_X.load(Ordering::SeqCst),
        y: DRAG_WINDOW_Y.load(Ordering::SeqCst),
        width: start_w,
        height: start_h,
    };
    let (x, y) = view::resized_origin(edge, start, width, height);
    SetWindowPos(hwnd, HWND_TOPMOST, x, y, width, height, SWP_NOACTIVATE);
    InvalidateRect(hwnd, std::ptr::null(), 0);
}

/// 结束拖动或缩放；松开鼠标时先把窗口夹回最近显示器的工作区，再通知主窗口保存。
unsafe fn finish_drag(hwnd: HWND, release: bool) {
    let edge = RESIZE_EDGE.swap(0, Ordering::SeqCst);
    let moved = DRAG_STARTED.swap(false, Ordering::SeqCst);
    let was_active = DRAG_ACTIVE.swap(false, Ordering::SeqCst);
    let start_x = DRAG_WINDOW_X.load(Ordering::SeqCst);
    let start_y = DRAG_WINDOW_Y.load(Ordering::SeqCst);
    let start_scale = RESIZE_START_SCALE.load(Ordering::SeqCst);
    if release && GetCapture() == hwnd {
        ReleaseCapture();
    }
    if !was_active {
        return;
    }
    if edge != 0 {
        let clamped = clamp_window(hwnd);
        let mut rect: RECT = std::mem::zeroed();
        if GetWindowRect(hwnd, &mut rect) != 0
            && (rect.left != start_x || rect.top != start_y || clamped)
        {
            notify_moved(hwnd);
        }
        let scale = BUBBLE_SCALE.load(Ordering::SeqCst);
        if scale != start_scale {
            SCALE_PENDING.store(true, Ordering::SeqCst);
            notify_scaled(scale);
        } else {
            SCALE_PENDING.store(false, Ordering::SeqCst);
        }
    } else if moved {
        // 普通拖动也必须保存，不能仅在夹取改变坐标时通知。
        clamp_window(hwnd);
        notify_moved(hwnd);
    }
}

unsafe fn cancel_drag(hwnd: HWND) {
    reset_drag_state();
    if GetCapture() == hwnd {
        ReleaseCapture();
    }
}

fn reset_drag_state() {
    DRAG_ACTIVE.store(false, Ordering::SeqCst);
    DRAG_STARTED.store(false, Ordering::SeqCst);
    RESIZE_EDGE.store(0, Ordering::SeqCst);
}

unsafe fn clamp_window(hwnd: HWND) -> bool {
    let mut rect: RECT = std::mem::zeroed();
    if GetWindowRect(hwnd, &mut rect) == 0 {
        return false;
    }
    let (width, height) =
        view::bubble_size(dpi_for_window(hwnd), BUBBLE_SCALE.load(Ordering::SeqCst));
    let area = work_area_for_window(hwnd);
    // 跨 DPI 拖动时仅调整尺寸，避免在鼠标仍按住时把窗口夹回屏幕边缘。
    let (x, y) = if DRAG_ACTIVE.load(Ordering::SeqCst) {
        (rect.left, rect.top)
    } else {
        view::clamp_bubble_position(rect.left, rect.top, width, height, area)
    };
    let resized = rect.right.saturating_sub(rect.left) != width
        || rect.bottom.saturating_sub(rect.top) != height;
    if x != rect.left || y != rect.top || resized {
        SetWindowPos(hwnd, HWND_TOPMOST, x, y, width, height, SWP_NOACTIVATE);
    }
    x != rect.left || y != rect.top
}

unsafe fn notify_moved(hwnd: HWND) {
    let main = MAIN_HWND.load(Ordering::SeqCst);
    let mut rect: RECT = std::mem::zeroed();
    if main != 0 && GetWindowRect(hwnd, &mut rect) != 0 {
        // 消息携带松手时的坐标，避免处理前窗口已隐藏或开始下一次拖动而读到错误位置。
        PostMessageW(
            main as HWND,
            super::WM_BUBBLE_MOVED,
            rect.left as WPARAM,
            rect.top as LPARAM,
        );
    }
}

unsafe fn notify_scaled(scale: u32) {
    let main = MAIN_HWND.load(Ordering::SeqCst);
    if main != 0 {
        PostMessageW(main as HWND, super::WM_BUBBLE_SCALED, scale as WPARAM, 0);
    }
}

const fn edge_code(edge: ResizeEdge) -> u32 {
    match edge {
        ResizeEdge::Left => 1,
        ResizeEdge::Right => 2,
        ResizeEdge::Top => 3,
        ResizeEdge::Bottom => 4,
        ResizeEdge::TopLeft => 5,
        ResizeEdge::TopRight => 6,
        ResizeEdge::BottomLeft => 7,
        ResizeEdge::BottomRight => 8,
    }
}

const fn edge_from_code(code: u32) -> Option<ResizeEdge> {
    match code {
        1 => Some(ResizeEdge::Left),
        2 => Some(ResizeEdge::Right),
        3 => Some(ResizeEdge::Top),
        4 => Some(ResizeEdge::Bottom),
        5 => Some(ResizeEdge::TopLeft),
        6 => Some(ResizeEdge::TopRight),
        7 => Some(ResizeEdge::BottomLeft),
        8 => Some(ResizeEdge::BottomRight),
        _ => None,
    }
}

fn system_dpi() -> u32 {
    match unsafe { GetDpiForSystem() } {
        0 => 96,
        dpi => dpi,
    }
}

fn dpi_for_window(hwnd: HWND) -> u32 {
    match unsafe { GetDpiForWindow(hwnd) } {
        0 => system_dpi(),
        dpi => dpi,
    }
}

fn effective_dpi(hwnd: HWND) -> u32 {
    view::bubble_dpi(dpi_for_window(hwnd), BUBBLE_SCALE.load(Ordering::SeqCst))
}

/// 缩放变化后调整窗口：自动位置重新贴回右上角；已保存坐标则保持左上角并夹回工作区。
unsafe fn apply_new_scale(hwnd: HWND, saved: Option<(i32, i32)>) {
    let dpi = dpi_for_window(hwnd);
    let scale = BUBBLE_SCALE.load(Ordering::SeqCst);
    let area = work_area_for_window(hwnd);
    let (x, y, width, height, notify) = match saved {
        None => {
            let rect = view::bubble_rect(dpi, scale, area, None);
            (rect.x, rect.y, rect.width, rect.height, false)
        }
        Some(_) => {
            let mut current: RECT = std::mem::zeroed();
            if GetWindowRect(hwnd, &mut current) == 0 {
                return;
            }
            let (width, height) = view::bubble_size(dpi, scale);
            let (x, y) =
                view::clamp_bubble_position(current.left, current.top, width, height, area);
            (x, y, width, height, x != current.left || y != current.top)
        }
    };
    SetWindowPos(hwnd, HWND_TOPMOST, x, y, width, height, SWP_NOACTIVATE);
    if notify {
        notify_moved(hwnd);
    }
}

fn work_area_for_point(point: (i32, i32)) -> WorkArea {
    let probe = RECT {
        left: point.0,
        top: point.1,
        right: point.0.saturating_add(1),
        bottom: point.1.saturating_add(1),
    };
    let monitor = unsafe { MonitorFromRect(&probe, MONITOR_DEFAULTTONEAREST) };
    unsafe { monitor_work_area(monitor) }
}

unsafe fn work_area_for_window(hwnd: HWND) -> WorkArea {
    let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
    unsafe { monitor_work_area(monitor) }
}

unsafe fn monitor_work_area(monitor: HMONITOR) -> WorkArea {
    let mut info: MONITORINFO = std::mem::zeroed();
    info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
    if !monitor.is_null() && GetMonitorInfoW(monitor, &mut info) != 0 {
        let area = WorkArea::new(
            info.rcWork.left,
            info.rcWork.top,
            info.rcWork.right,
            info.rcWork.bottom,
        );
        if area.width() > 0 && area.height() > 0 {
            return area;
        }
    }
    primary_work_area()
}

fn primary_work_area() -> WorkArea {
    unsafe {
        let mut area: RECT = std::mem::zeroed();
        let ok = SystemParametersInfoW(
            SPI_GETWORKAREA,
            0,
            &mut area as *mut RECT as *mut std::ffi::c_void,
            0,
        );
        if ok != 0 && area.right > area.left && area.bottom > area.top {
            return WorkArea::new(area.left, area.top, area.right, area.bottom);
        }
        WorkArea::new(
            0,
            0,
            GetSystemMetrics(SM_CXSCREEN),
            GetSystemMetrics(SM_CYSCREEN),
        )
    }
}

unsafe fn paint(hwnd: HWND) {
    let mut paint_struct: PAINTSTRUCT = std::mem::zeroed();
    let hdc = BeginPaint(hwnd, &mut paint_struct);
    if hdc.is_null() {
        return;
    }

    let mut client: RECT = std::mem::zeroed();
    if GetClientRect(hwnd, &mut client) == 0 {
        EndPaint(hwnd, &paint_struct);
        return;
    }
    let width = client.right.saturating_sub(client.left);
    let height = client.bottom.saturating_sub(client.top);
    if width <= 0 || height <= 0 {
        EndPaint(hwnd, &paint_struct);
        return;
    }

    let memory_dc = CreateCompatibleDC(hdc);
    if memory_dc.is_null() {
        EndPaint(hwnd, &paint_struct);
        return;
    }
    let bitmap = CreateCompatibleBitmap(hdc, width, height);
    if bitmap.is_null() {
        DeleteDC(memory_dc);
        EndPaint(hwnd, &paint_struct);
        return;
    }
    let old_bitmap = SelectObject(memory_dc, bitmap);

    let key = CreateSolidBrush(key_color());
    if !key.is_null() {
        FillRect(memory_dc, &client, key);
        DeleteObject(key);
    }

    let radius = radius_for(height);
    // GDI 接收的是圆角椭圆的直径，与命中测试使用同一个半径。
    let region = CreateRoundRectRgn(0, 0, width + 1, height + 1, radius * 2, radius * 2);
    let paused = PAUSED.load(Ordering::SeqCst);
    let palette =
        view::bubble_palette(theme_from_code(BUBBLE_THEME.load(Ordering::SeqCst)), paused);
    let background_color = rgb_tuple(palette.background);
    let border = rgb_tuple(palette.border);
    let dpi = effective_dpi(hwnd);
    let scaled = |value| view::scale_for_dpi(value, dpi);

    if !region.is_null() {
        let background = CreateSolidBrush(background_color);
        if !background.is_null() {
            FillRgn(memory_dc, region, background);
            DeleteObject(background);
        }
        let border_brush = CreateSolidBrush(border);
        if !border_brush.is_null() {
            let border_width = scaled(1).max(1);
            FrameRgn(memory_dc, region, border_brush, border_width, border_width);
            DeleteObject(border_brush);
        }
        DeleteObject(region);
    }

    // 文字和右侧品牌图标分别占用固定区域，最长配置倒计时 600:00 也能完整显示。
    let pad = scaled(view::BUBBLE_PAD);
    let icon_size = scaled(view::BUBBLE_ICON);
    let icon_left = width - pad - icon_size;
    let text_right = pad + view::bubble_text_width(width, dpi);
    let font_size = scaled(13);
    let lang = crate::i18n::current();
    let clock = crate::view::format_clock(REMAINING_MS.load(Ordering::SeqCst));
    let (text, color) = if paused {
        (crate::i18n::bubble_paused(lang, &clock), palette.status)
    } else {
        (
            crate::i18n::bubble_countdown(lang, &clock),
            palette.countdown,
        )
    };
    let text_rect = RECT {
        left: pad,
        top: scaled(2),
        right: text_right,
        bottom: height - scaled(2),
    };
    draw_text(
        memory_dc,
        &text,
        &text_rect,
        font_size,
        FW_NORMAL as i32,
        rgb_tuple(color),
    );

    // 复用嵌入的 Liftly 图标；AppIcon 在绘制结束时释放自有句柄，共享兜底图标不销毁。
    let icon = AppIcon::load(icon_size, icon_size);
    if !icon.handle.is_null() {
        DrawIconEx(
            memory_dc,
            icon_left,
            (height - icon_size) / 2,
            icon.handle,
            icon_size,
            icon_size,
            0,
            std::ptr::null_mut(),
            DI_NORMAL,
        );
    }

    BitBlt(hdc, 0, 0, width, height, memory_dc, 0, 0, SRCCOPY);

    SelectObject(memory_dc, old_bitmap);
    DeleteObject(bitmap);
    DeleteDC(memory_dc);
    EndPaint(hwnd, &paint_struct);
}

fn radius_for(height: i32) -> i32 {
    (BASE_RADIUS * height / view::BUBBLE_BASE_HEIGHT).max(1)
}

unsafe fn draw_text(hdc: HDC, text: &str, rect: &RECT, size: i32, weight: i32, color: u32) {
    if size <= 0 || rect.right <= rect.left || rect.bottom <= rect.top {
        return;
    }
    let face = wide("Microsoft YaHei UI");
    let font = CreateFontW(
        // 负高度指定实际字形高度，避免不同 DPI 下字符偏小。
        -size,
        0,
        0,
        0,
        weight,
        0,
        0,
        0,
        u32::from(DEFAULT_CHARSET),
        u32::from(OUT_DEFAULT_PRECIS),
        u32::from(CLIP_DEFAULT_PRECIS),
        u32::from(CLEARTYPE_QUALITY),
        u32::from(DEFAULT_PITCH | FF_DONTCARE),
        face.as_ptr(),
    );
    if font.is_null() {
        return;
    }

    let old_font = SelectObject(hdc, font);
    SetTextColor(hdc, color);
    SetBkMode(hdc, TRANSPARENT as i32);

    let mut text_rect = *rect;
    let text = wide(text);
    DrawTextW(
        hdc,
        text.as_ptr(),
        -1,
        &mut text_rect,
        DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
    );

    SelectObject(hdc, old_font);
    DeleteObject(font);
}

const fn key_color() -> u32 {
    rgb(255, 0, 255)
}

const fn rgb(red: u8, green: u8, blue: u8) -> u32 {
    (red as u32) | ((green as u32) << 8) | ((blue as u32) << 16)
}

const fn rgb_tuple((red, green, blue): (u8, u8, u8)) -> u32 {
    rgb(red, green, blue)
}

const fn theme_code(theme: BubbleTheme) -> u32 {
    match theme {
        BubbleTheme::Green => 0,
        BubbleTheme::Blue => 1,
        BubbleTheme::Orange => 2,
        BubbleTheme::Dark => 3,
    }
}

const fn theme_from_code(code: u32) -> BubbleTheme {
    match code {
        1 => BubbleTheme::Blue,
        2 => BubbleTheme::Orange,
        3 => BubbleTheme::Dark,
        _ => BubbleTheme::Green,
    }
}
