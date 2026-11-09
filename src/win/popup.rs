//! 锁定前的右下角提醒弹窗。每秒刷新剩余秒数，点击后自行关闭，不抢焦点。

use std::sync::atomic::{AtomicIsize, AtomicU64, Ordering};
use std::sync::Once;

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    BeginPaint, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, CreateFontW, CreateSolidBrush,
    DeleteDC, DeleteObject, DrawTextW, EndPaint, FillRect, FrameRect, InvalidateRect, SelectObject,
    SetBkMode, SetTextColor, CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET,
    DEFAULT_PITCH, DT_END_ELLIPSIS, DT_LEFT, DT_NOPREFIX, DT_SINGLELINE, DT_VCENTER, FF_DONTCARE,
    FW_BOLD, FW_NORMAL, HDC, OUT_DEFAULT_PRECIS, PAINTSTRUCT, SRCCOPY, TRANSPARENT,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::SystemInformation::GetTickCount64;
use windows_sys::Win32::UI::HiDpi::GetDpiForSystem;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetClientRect, GetSystemMetrics, LoadCursorW,
    RegisterClassW, SetWindowPos, ShowWindow, SystemParametersInfoW, HWND_TOPMOST, IDC_ARROW,
    MA_NOACTIVATE, SM_CXSCREEN, SM_CYSCREEN, SPI_GETWORKAREA, SWP_NOACTIVATE, SWP_NOMOVE,
    SWP_NOSIZE, SW_SHOWNOACTIVATE, WM_CLOSE, WM_DESTROY, WM_ERASEBKGND, WM_LBUTTONUP,
    WM_MOUSEACTIVATE, WM_PAINT, WNDCLASSW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
    WS_POPUP,
};

use super::wide;

const POPUP_CLASS: &str = "Liftly.WarnPopup";
const BASE_WIDTH: i32 = 360;
const BASE_HEIGHT: i32 = 128;
const BASE_MARGIN: i32 = 16;

static CLASS_REGISTERED: Once = Once::new();
static WARN_DEADLINE_MS: AtomicU64 = AtomicU64::new(0);
static POPUP_HWND: AtomicIsize = AtomicIsize::new(0);

/// 提醒弹窗。窗口句柄统一保存在 `POPUP_HWND` 中，避免结构体继续操作已销毁的句柄。
pub struct Popup;

impl Popup {
    pub fn new() -> Self {
        Self
    }

    /// 保存截止时间并创建弹窗；已存在时先销毁旧窗口。返回窗口是否创建成功。
    pub fn show(&mut self, deadline_ms: u64) -> bool {
        if !current_hwnd().is_null() {
            self.hide();
        }
        WARN_DEADLINE_MS.store(deadline_ms, Ordering::SeqCst);

        ensure_class();
        let hwnd = unsafe { create_popup_window() };
        if hwnd.is_null() {
            WARN_DEADLINE_MS.store(0, Ordering::SeqCst);
            return false;
        }

        POPUP_HWND.store(hwnd as isize, Ordering::SeqCst);
        unsafe {
            ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        }
        true
    }

    /// 每秒刷新倒计时，并重新压到所有置顶窗口之上。
    pub fn refresh(&self) {
        let hwnd = current_hwnd();
        if hwnd.is_null() {
            return;
        }
        unsafe {
            InvalidateRect(hwnd, std::ptr::null(), 0);
            SetWindowPos(
                hwnd,
                HWND_TOPMOST,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
        }
    }

    /// 关闭弹窗；可重复调用。用户点击关闭后同样会通过 `WM_DESTROY` 清空句柄。
    pub fn hide(&mut self) {
        let hwnd = current_hwnd();
        if !hwnd.is_null() {
            unsafe {
                DestroyWindow(hwnd);
            }
            // DestroyWindow 会同步触发 WM_DESTROY；这里再兜底清一次，防止句柄残留。
            let _ =
                POPUP_HWND.compare_exchange(hwnd as isize, 0, Ordering::SeqCst, Ordering::SeqCst);
        }
        WARN_DEADLINE_MS.store(0, Ordering::SeqCst);
    }

    pub fn is_visible(&self) -> bool {
        !current_hwnd().is_null()
    }
}

fn current_hwnd() -> HWND {
    POPUP_HWND.load(Ordering::SeqCst) as HWND
}

fn ensure_class() {
    CLASS_REGISTERED.call_once(|| unsafe {
        let hinstance = GetModuleHandleW(std::ptr::null());
        let class = wide(POPUP_CLASS);
        let wc = WNDCLASSW {
            style: 0,
            lpfnWndProc: Some(popup_wnd_proc),
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

/// 按系统 DPI 计算基准尺寸和右下角位置；工作区获取失败时退回整个主屏幕。
unsafe fn create_popup_window() -> HWND {
    let dpi = match GetDpiForSystem() {
        0 => 96,
        value => value,
    };
    let scale = |value: i32| (value * dpi as i32 + 48) / 96;
    let width = scale(BASE_WIDTH).max(1);
    let height = scale(BASE_HEIGHT).max(1);
    let margin = scale(BASE_MARGIN);

    let mut area: RECT = std::mem::zeroed();
    let ok = SystemParametersInfoW(
        SPI_GETWORKAREA,
        0,
        &mut area as *mut RECT as *mut std::ffi::c_void,
        0,
    );
    if ok == 0 || area.right <= area.left || area.bottom <= area.top {
        area = RECT {
            left: 0,
            top: 0,
            right: GetSystemMetrics(SM_CXSCREEN),
            bottom: GetSystemMetrics(SM_CYSCREEN),
        };
    }

    let x = area
        .right
        .saturating_sub(width)
        .saturating_sub(margin)
        .max(area.left);
    let y = area
        .bottom
        .saturating_sub(height)
        .saturating_sub(margin)
        .max(area.top);

    let hinstance = GetModuleHandleW(std::ptr::null());
    let class = wide(POPUP_CLASS);
    let title = wide("站会儿");
    CreateWindowExW(
        WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
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

unsafe extern "system" fn popup_wnd_proc(
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
        WM_MOUSEACTIVATE => MA_NOACTIVATE as LRESULT,
        WM_LBUTTONUP => {
            // 点击提示即关闭，不进入拖动或焦点流程。
            DestroyWindow(hwnd);
            0
        }
        WM_CLOSE => {
            DestroyWindow(hwnd);
            0
        }
        WM_DESTROY => {
            if POPUP_HWND.load(Ordering::SeqCst) == hwnd as isize {
                POPUP_HWND.store(0, Ordering::SeqCst);
                WARN_DEADLINE_MS.store(0, Ordering::SeqCst);
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
    fill_background(memory_dc, &client);

    // 左右内边距按窗口宽度等比缩放，保证高 DPI 下文字不贴边。
    let pad = (BASE_MARGIN * height / BASE_HEIGHT).max(8);
    let left = pad.min(width / 2);
    let right = width.saturating_sub(pad).max(left + 1);

    let small_size = (height * 15 / BASE_HEIGHT).max(10);
    let big_size = (height * 22 / BASE_HEIGHT).max(12);
    let tiny_size = (height * 14 / BASE_HEIGHT).max(10);

    let first = RECT {
        left,
        top: height * 13 / 100,
        right,
        bottom: height * 33 / 100,
    };
    draw_text(
        memory_dc,
        "站会儿 · 即将休息",
        &first,
        small_size,
        FW_NORMAL as i32,
        rgb(150, 162, 174),
    );

    let remaining = WARN_DEADLINE_MS
        .load(Ordering::SeqCst)
        .saturating_sub(GetTickCount64());
    let secs = remaining.div_ceil(1_000);
    let countdown_text = if secs == 0 {
        "马上开始休息".to_owned()
    } else {
        format!("{} 秒后锁定，站起来活动一下", secs)
    };
    let second = RECT {
        left,
        top: height * 33 / 100,
        right,
        bottom: height * 69 / 100,
    };
    draw_text(
        memory_dc,
        &countdown_text,
        &second,
        big_size,
        FW_BOLD as i32,
        rgb(238, 243, 248),
    );

    let third = RECT {
        left,
        top: height * 70 / 100,
        right,
        bottom: height * 90 / 100,
    };
    draw_text(
        memory_dc,
        "请保存手头工作 · 点击关闭此提示",
        &third,
        tiny_size,
        FW_NORMAL as i32,
        rgb(150, 162, 174),
    );

    BitBlt(hdc, 0, 0, width, height, memory_dc, 0, 0, SRCCOPY);

    SelectObject(memory_dc, old_bitmap);
    DeleteObject(bitmap);
    DeleteDC(memory_dc);
    EndPaint(hwnd, &paint_struct);
}

unsafe fn fill_background(hdc: HDC, rect: &RECT) {
    let brush = CreateSolidBrush(rgb(32, 38, 46));
    if !brush.is_null() {
        FillRect(hdc, rect, brush);
        DeleteObject(brush);
    }

    let border = CreateSolidBrush(rgb(112, 198, 255));
    if !border.is_null() {
        FrameRect(hdc, rect, border);
        DeleteObject(border);
    }
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

const fn rgb(red: u8, green: u8, blue: u8) -> u32 {
    (red as u32) | ((green as u32) << 8) | ((blue as u32) << 16)
}
