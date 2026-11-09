//! 桌面倒计时气泡。浅绿色双行圆角卡片，可拖动并按显示器工作区夹取。

use std::sync::atomic::{AtomicBool, AtomicI32, AtomicIsize, AtomicU64, Ordering};
use std::sync::Once;

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    BeginPaint, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, CreateFontW, CreateRoundRectRgn,
    CreateSolidBrush, DeleteDC, DeleteObject, DrawTextW, EndPaint, FillRect, FillRgn, FrameRgn,
    GetMonitorInfoW, InvalidateRect, MonitorFromRect, MonitorFromWindow, SelectObject, SetBkMode,
    SetTextColor, CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET, DEFAULT_PITCH,
    DT_LEFT, DT_NOPREFIX, DT_SINGLELINE, DT_VCENTER, FF_DONTCARE, FW_NORMAL, HDC, HMONITOR,
    MONITORINFO, MONITOR_DEFAULTTONEAREST, OUT_DEFAULT_PRECIS, PAINTSTRUCT, SRCCOPY, TRANSPARENT,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::HiDpi::{GetDpiForSystem, GetDpiForWindow};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetCapture, ReleaseCapture, SetCapture};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DrawIconEx, GetClientRect, GetCursorPos,
    GetSystemMetrics, GetWindowRect, IsWindow, LoadCursorW, PostMessageW, RegisterClassW,
    SetLayeredWindowAttributes, SetWindowPos, ShowWindow, SystemParametersInfoW, DI_NORMAL,
    HTCLIENT, HTTRANSPARENT, HWND_TOPMOST, IDC_SIZEALL, LWA_ALPHA, LWA_COLORKEY, MA_NOACTIVATE,
    SM_CXSCREEN, SM_CYSCREEN,
    SPI_GETWORKAREA, SWP_NOACTIVATE, SWP_NOSIZE, SW_SHOWNOACTIVATE, WM_CAPTURECHANGED,
    WM_CLOSE, WM_DESTROY, WM_DPICHANGED, WM_ERASEBKGND, WM_LBUTTONDOWN, WM_LBUTTONUP,
    WM_MOUSEACTIVATE, WM_MOUSEMOVE, WM_NCHITTEST, WM_PAINT, WNDCLASSW, WS_EX_LAYERED,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};

use super::{wide, AppIcon};
use crate::view::{self, WorkArea};

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
    pub fn show(&mut self, remaining_ms: u64, paused: bool, saved: Option<(i32, i32)>) -> bool {
        REMAINING_MS.store(remaining_ms, Ordering::SeqCst);
        PAUSED.store(paused, Ordering::SeqCst);

        let mut hwnd = current_hwnd();
        if hwnd.is_null() {
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
                // 创建时按系统 DPI 估算；再按窗口实际 DPI 校正尺寸、边距和保存位置。
                let rect = view::bubble_rect(dpi_for_window(hwnd), work_area_for_window(hwnd), saved);
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
            // 鼠标悬停卡片时提示可以拖动。
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
    // 有保存坐标时在坐标所在的显示器上夹取，支持副屏负坐标；否则使用主屏工作区。
    let area = match saved {
        Some(point) => work_area_for_point(point),
        None => primary_work_area(),
    };
    let rect = view::bubble_rect(dpi, area, saved);

    let hinstance = GetModuleHandleW(std::ptr::null());
    let class = wide(BUBBLE_CLASS);
    let title = wide("站会儿倒计时");
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
        WM_LBUTTONDOWN => {
            begin_drag(hwnd);
            0
        }
        WM_MOUSEMOVE => {
            drag_to_cursor(hwnd);
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
    corners
        .into_iter()
        .any(|(in_corner, center_x, center_y)| {
            if !in_corner {
                return false;
            }
            let dx = x.saturating_sub(center_x);
            let dy = y.saturating_sub(center_y);
            dx.saturating_mul(dx).saturating_add(dy.saturating_mul(dy)) > radius_squared
        })
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
    SetWindowPos(
        hwnd,
        HWND_TOPMOST,
        x,
        y,
        0,
        0,
        SWP_NOSIZE | SWP_NOACTIVATE,
    );
}

/// 结束拖动；松开鼠标时先把窗口夹回最近显示器的工作区，再通知主窗口保存坐标。
unsafe fn finish_drag(hwnd: HWND, release: bool) {
    let moved = DRAG_STARTED.swap(false, Ordering::SeqCst);
    let was_active = DRAG_ACTIVE.swap(false, Ordering::SeqCst);
    if release && GetCapture() == hwnd {
        ReleaseCapture();
    }
    if was_active && moved {
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
}

unsafe fn clamp_window(hwnd: HWND) -> bool {
    let mut rect: RECT = std::mem::zeroed();
    if GetWindowRect(hwnd, &mut rect) == 0 {
        return false;
    }
    let (width, height) = view::bubble_size(dpi_for_window(hwnd));
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
        SetWindowPos(
            hwnd,
            HWND_TOPMOST,
            x,
            y,
            width,
            height,
            SWP_NOACTIVATE,
        );
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
    let (background_color, border, status_color) = if paused {
        (rgb(255, 248, 225), rgb(199, 162, 91), rgb(143, 99, 27))
    } else {
        (rgb(239, 248, 225), rgb(115, 181, 83), rgb(58, 104, 48))
    };
    let dpi = dpi_for_window(hwnd);
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
    let pad = scaled(9);
    let icon_size = scaled(28);
    let icon_left = width - pad - icon_size;
    let text_right = icon_left - scaled(7);
    let font_size = scaled(13);
    let clock = crate::view::format_clock(REMAINING_MS.load(Ordering::SeqCst));
    let countdown = format!("距休息 {clock}");
    let countdown_rect = RECT {
        left: pad,
        top: scaled(4),
        right: text_right,
        bottom: height / 2,
    };
    draw_text(
        memory_dc,
        &countdown,
        &countdown_rect,
        font_size,
        FW_NORMAL as i32,
        rgb(43, 82, 43),
    );

    let status_rect = RECT {
        left: pad,
        top: height / 2,
        right: text_right,
        bottom: height - scaled(4),
    };
    draw_text(
        memory_dc,
        if paused { "已暂停" } else { "工作中" },
        &status_rect,
        font_size,
        FW_NORMAL as i32,
        status_color,
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
