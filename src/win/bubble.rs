//! 桌面右上角的倒计时气泡。置顶、不抢焦点、鼠标穿透，工作区变化时重新定位。

use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU64, Ordering};
use std::sync::Once;

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    BeginPaint, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, CreateFontW,
    CreateRoundRectRgn, CreateSolidBrush, DeleteDC, DeleteObject, DrawTextW, EndPaint, FillRect,
    FillRgn, FrameRgn, InvalidateRect, SelectObject, SetBkMode, SetTextColor, CLEARTYPE_QUALITY,
    CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET, DEFAULT_PITCH, DT_END_ELLIPSIS, DT_LEFT, DT_NOPREFIX,
    DT_SINGLELINE, DT_VCENTER, FF_DONTCARE, FW_BOLD, FW_NORMAL, HDC, OUT_DEFAULT_PRECIS,
    PAINTSTRUCT, SRCCOPY, TRANSPARENT,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::HiDpi::GetDpiForSystem;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetClientRect, GetSystemMetrics, IsWindow,
    LoadCursorW, RegisterClassW, SetLayeredWindowAttributes, SetWindowPos, ShowWindow,
    SystemParametersInfoW, HTTRANSPARENT, HWND_TOPMOST, IDC_ARROW, LWA_ALPHA, LWA_COLORKEY,
    MA_NOACTIVATE, SM_CXSCREEN, SM_CYSCREEN, SPI_GETWORKAREA, SWP_NOACTIVATE, SW_SHOWNOACTIVATE,
    WM_CLOSE, WM_DESTROY, WM_ERASEBKGND, WM_MOUSEACTIVATE, WM_NCHITTEST, WM_PAINT, WNDCLASSW,
    WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};

use super::wide;

const BUBBLE_CLASS: &str = "Liftly.CountdownBubble";
const BASE_WIDTH: i32 = 190;
const BASE_HEIGHT: i32 = 78;
const BASE_MARGIN: i32 = 20;
const BASE_RADIUS: i32 = 14;
const ALPHA: u8 = 236;

static CLASS_REGISTERED: Once = Once::new();
static BUBBLE_HWND: AtomicIsize = AtomicIsize::new(0);
static REMAINING_MS: AtomicU64 = AtomicU64::new(0);
static PAUSED: AtomicBool = AtomicBool::new(false);

/// 倒计时气泡。窗口句柄统一保存在静态变量中，避免结构体继续操作已销毁的句柄。
#[derive(Default)]
pub struct Bubble {
    failed: bool,
}

impl Bubble {
    pub fn new() -> Self {
        Self::default()
    }

    /// 更新显示内容；窗口不存在时按当前 DPI 创建。返回窗口是否可用。
    pub fn show(&mut self, remaining_ms: u64, paused: bool) -> bool {
        REMAINING_MS.store(remaining_ms, Ordering::SeqCst);
        PAUSED.store(paused, Ordering::SeqCst);

        let hwnd = current_hwnd();
        if hwnd.is_null() {
            ensure_class();
            let hwnd = unsafe { create_bubble_window() };
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
                ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            }
            self.failed = false;
        }

        unsafe {
            InvalidateRect(hwnd, std::ptr::null(), 0);
            // 每次刷新都重新定位并置顶，兼顾任务栏/分辨率/DISPLAYCHANGE 变化。
            let (x, y, width, height) = geometry();
            SetWindowPos(hwnd, HWND_TOPMOST, x, y, width, height, SWP_NOACTIVATE);
        }
        true
    }

    /// 关闭气泡；可重复调用。
    pub fn hide(&mut self) {
        let hwnd = current_hwnd();
        if !hwnd.is_null() {
            unsafe {
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
            hCursor: LoadCursorW(std::ptr::null_mut(), IDC_ARROW),
            hbrBackground: std::ptr::null_mut(),
            lpszMenuName: std::ptr::null(),
            lpszClassName: class.as_ptr(),
        };
        RegisterClassW(&wc);
    });
}

/// 按系统 DPI 计算尺寸，并固定主屏工作区右上角；工作区获取失败时退回整个主屏幕。
fn geometry() -> (i32, i32, i32, i32) {
    let dpi = match unsafe { GetDpiForSystem() } {
        0 => 96,
        value => value,
    };
    let scale = |value: i32| (value * dpi as i32 + 48) / 96;
    let width = scale(BASE_WIDTH).max(1);
    let height = scale(BASE_HEIGHT).max(1);
    let margin = scale(BASE_MARGIN);

    let mut area: RECT = unsafe { std::mem::zeroed() };
    let ok = unsafe {
        SystemParametersInfoW(
            SPI_GETWORKAREA,
            0,
            &mut area as *mut RECT as *mut std::ffi::c_void,
            0,
        )
    };
    if ok == 0 || area.right <= area.left || area.bottom <= area.top {
        area = unsafe {
            RECT {
                left: 0,
                top: 0,
                right: GetSystemMetrics(SM_CXSCREEN),
                bottom: GetSystemMetrics(SM_CYSCREEN),
            }
        };
    }

    let x = area
        .right
        .saturating_sub(width)
        .saturating_sub(margin)
        .max(area.left);
    let y = area
        .top
        .saturating_add(margin)
        .min(area.bottom.saturating_sub(height))
        .max(area.top);
    (x, y, width, height)
}

unsafe fn create_bubble_window() -> HWND {
    let (x, y, width, height) = geometry();
    let hinstance = GetModuleHandleW(std::ptr::null());
    let class = wide(BUBBLE_CLASS);
    let title = wide("站会儿倒计时");
    CreateWindowExW(
        WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_LAYERED | WS_EX_TRANSPARENT,
        class.as_ptr(),
        title.as_ptr(),
        WS_POPUP,
        x,
        y,
        width,
        height,
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
        // 双重保险：即使分层窗口的透明区域没有挡住命中测试，也不接收鼠标。
        WM_NCHITTEST => HTTRANSPARENT as LRESULT,
        WM_MOUSEACTIVATE => MA_NOACTIVATE as LRESULT,
        WM_CLOSE => {
            DestroyWindow(hwnd);
            0
        }
        WM_DESTROY => {
            if BUBBLE_HWND.load(Ordering::SeqCst) == hwnd as isize {
                BUBBLE_HWND.store(0, Ordering::SeqCst);
            }
            0
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
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

    let radius = (BASE_RADIUS * height / BASE_HEIGHT).max(4);
    let region = CreateRoundRectRgn(0, 0, width + 1, height + 1, radius, radius);
    let paused = PAUSED.load(Ordering::SeqCst);
    let border_color = if paused {
        rgb(245, 185, 66)
    } else {
        rgb(61, 220, 151)
    };

    if !region.is_null() {
        let background = CreateSolidBrush(rgb(30, 36, 44));
        if !background.is_null() {
            FillRgn(memory_dc, region, background);
            DeleteObject(background);
        }
        let border = CreateSolidBrush(border_color);
        if !border.is_null() {
            FrameRgn(
                memory_dc,
                region,
                border,
                (height / BASE_HEIGHT).max(1),
                (height / BASE_HEIGHT).max(1),
            );
            DeleteObject(border);
        }
        DeleteObject(region);
    }

    let pad = (14 * width / BASE_WIDTH).max(8);
    let label_size = (height * 15 / BASE_HEIGHT).max(10);
    let clock_size = (height * 32 / BASE_HEIGHT).max(12);
    let label = if paused { "已暂停" } else { "距休息" };
    let clock = crate::view::format_clock(REMAINING_MS.load(Ordering::SeqCst));

    let label_rect = RECT {
        left: pad,
        top: height * 8 / 100,
        right: width.saturating_sub(pad).max(pad + 1),
        bottom: height * 40 / 100,
    };
    draw_text(
        memory_dc,
        label,
        &label_rect,
        label_size,
        FW_NORMAL as i32,
        rgb(150, 162, 174),
    );

    let clock_rect = RECT {
        left: pad,
        top: height * 36 / 100,
        right: width.saturating_sub(pad).max(pad + 1),
        bottom: height * 92 / 100,
    };
    let clock_color = if paused {
        rgb(255, 205, 120)
    } else {
        rgb(238, 243, 248)
    };
    draw_text(
        memory_dc,
        &clock,
        &clock_rect,
        clock_size,
        FW_BOLD as i32,
        clock_color,
    );

    BitBlt(hdc, 0, 0, width, height, memory_dc, 0, 0, SRCCOPY);

    SelectObject(memory_dc, old_bitmap);
    DeleteObject(bitmap);
    DeleteDC(memory_dc);
    EndPaint(hwnd, &paint_struct);
}

unsafe fn draw_text(hdc: HDC, text: &str, rect: &RECT, size: i32, weight: i32, color: u32) {
    if size <= 0 || rect.right <= rect.left || rect.bottom <= rect.top {
        return;
    }
    let face = wide("Microsoft YaHei UI");
    let font = CreateFontW(
        size,
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
        DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX,
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
