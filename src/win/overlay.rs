//! 全屏遮罩窗口。每个显示器一个顶层置顶窗口，GDI 双缓冲绘制深色背景和倒计时。

use std::mem::size_of;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Once;

use windows_sys::Win32::Foundation::{BOOL, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    BeginPaint, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, CreateFontW, CreateSolidBrush,
    DeleteDC, DeleteObject, DrawTextW, EndPaint, EnumDisplayMonitors, FillRect, GetMonitorInfoW,
    InvalidateRect, SelectObject, SetBkMode, SetTextColor, CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS,
    DEFAULT_CHARSET, DEFAULT_PITCH, DT_CENTER, DT_NOPREFIX, DT_SINGLELINE, DT_VCENTER, FF_DONTCARE,
    FW_BOLD, HDC, HMONITOR, MONITORINFO, OUT_DEFAULT_PRECIS, PAINTSTRUCT, SRCCOPY, TRANSPARENT,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::SystemInformation::GetTickCount64;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetClientRect, GetSystemMetrics, LoadCursorW,
    RegisterClassW, SetWindowPos, HWND_TOPMOST, IDC_ARROW, SM_CXSCREEN, SM_CXVIRTUALSCREEN,
    SM_CYSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, SWP_NOACTIVATE,
    SWP_NOMOVE, SWP_NOSIZE, WM_CLOSE, WM_ERASEBKGND, WM_PAINT, WNDCLASSW, WS_EX_TOOLWINDOW,
    WS_EX_TOPMOST, WS_POPUP, WS_VISIBLE,
};

use super::wide;

const OVERLAY_CLASS: &str = "Liftly.Overlay";

static CLASS_REGISTERED: Once = Once::new();
static BREAK_DEADLINE_MS: AtomicU64 = AtomicU64::new(0);
static EMERGENCY_ENABLED: AtomicBool = AtomicBool::new(false);

#[derive(Default)]
pub struct Overlay {
    windows: Vec<HWND>,
}

impl Overlay {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn start(&mut self, deadline_ms: u64, emergency_unlock: bool) {
        if !self.windows.is_empty() {
            self.stop();
        }
        ensure_class();

        BREAK_DEADLINE_MS.store(deadline_ms, Ordering::SeqCst);
        EMERGENCY_ENABLED.store(emergency_unlock, Ordering::SeqCst);

        let mut monitors = unsafe { collect_monitors() };
        if monitors.is_empty() {
            monitors.push(fallback_monitor());
        }

        for rect in monitors {
            let hwnd = unsafe { create_overlay_window(rect) };
            if !hwnd.is_null() {
                self.windows.push(hwnd);
            }
        }
    }

    pub fn stop(&mut self) {
        unsafe {
            for hwnd in self.windows.drain(..) {
                DestroyWindow(hwnd);
            }
        }
        BREAK_DEADLINE_MS.store(0, Ordering::SeqCst);
        EMERGENCY_ENABLED.store(false, Ordering::SeqCst);
    }

    /// 每秒刷新倒计时，并重新压到所有置顶窗口之上。
    pub fn refresh(&self) {
        unsafe {
            for &hwnd in &self.windows {
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
    }

    pub fn rebuild(&mut self, deadline_ms: u64, emergency_unlock: bool) {
        self.stop();
        self.start(deadline_ms, emergency_unlock);
    }
}

fn ensure_class() {
    CLASS_REGISTERED.call_once(|| unsafe {
        let hinstance = GetModuleHandleW(std::ptr::null());
        let class = wide(OVERLAY_CLASS);
        let wc = WNDCLASSW {
            style: 0,
            lpfnWndProc: Some(overlay_wnd_proc),
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

unsafe fn collect_monitors() -> Vec<RECT> {
    let mut monitors: Vec<RECT> = Vec::new();
    let data = &mut monitors as *mut Vec<RECT> as LPARAM;
    let ok = EnumDisplayMonitors(
        std::ptr::null_mut(),
        std::ptr::null(),
        Some(monitor_enum_proc),
        data,
    );
    if ok == 0 {
        monitors.clear();
    }
    monitors
}

unsafe extern "system" fn monitor_enum_proc(
    monitor: HMONITOR,
    _hdc: HDC,
    _rect: *mut RECT,
    data: LPARAM,
) -> BOOL {
    let mut info: MONITORINFO = std::mem::zeroed();
    info.cbSize = size_of::<MONITORINFO>() as u32;
    if GetMonitorInfoW(monitor, &mut info) != 0 {
        (*(data as *mut Vec<RECT>)).push(info.rcMonitor);
    }
    1
}

fn fallback_monitor() -> RECT {
    unsafe {
        let mut width = GetSystemMetrics(SM_CXVIRTUALSCREEN);
        let mut height = GetSystemMetrics(SM_CYVIRTUALSCREEN);
        let mut left = GetSystemMetrics(SM_XVIRTUALSCREEN);
        let mut top = GetSystemMetrics(SM_YVIRTUALSCREEN);
        if width <= 0 || height <= 0 {
            width = GetSystemMetrics(SM_CXSCREEN);
            height = GetSystemMetrics(SM_CYSCREEN);
            left = 0;
            top = 0;
        }
        RECT {
            left,
            top,
            right: left.saturating_add(width),
            bottom: top.saturating_add(height),
        }
    }
}

unsafe fn create_overlay_window(rect: RECT) -> HWND {
    let width = rect.right.saturating_sub(rect.left);
    let height = rect.bottom.saturating_sub(rect.top);
    if width <= 0 || height <= 0 {
        return std::ptr::null_mut();
    }

    let hinstance = GetModuleHandleW(std::ptr::null());
    let class = wide(OVERLAY_CLASS);
    let title = wide(crate::i18n::overlay_window_title(crate::i18n::current()));
    let hwnd = CreateWindowExW(
        WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
        class.as_ptr(),
        title.as_ptr(),
        WS_POPUP | WS_VISIBLE,
        rect.left,
        rect.top,
        width,
        height,
        std::ptr::null_mut(),
        std::ptr::null_mut(),
        hinstance,
        std::ptr::null(),
    );
    if !hwnd.is_null() {
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
    hwnd
}

unsafe extern "system" fn overlay_wnd_proc(
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
        // 遮罩不允许被关闭；退出流程由主窗口控制。
        WM_CLOSE => 0,
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

struct TextLine<'a> {
    text: &'a str,
    size: i32,
    weight: i32,
    color: u32,
    top: i32,
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

    let title_size = (height / 10).clamp(24, 220);
    let countdown_size = (height / 20).clamp(20, 120);
    let hint_size = (height / 40).clamp(14, 60);

    let lang = crate::i18n::current();
    let title_text = crate::i18n::overlay_title(lang);
    let title = TextLine {
        text: title_text,
        size: title_size,
        weight: FW_BOLD as i32,
        color: rgb(238, 243, 248),
        top: height * 38 / 100,
    };
    draw_line(memory_dc, &title, width);

    let remaining = BREAK_DEADLINE_MS
        .load(Ordering::SeqCst)
        .saturating_sub(GetTickCount64());
    let countdown_text = crate::i18n::overlay_countdown(lang, remaining);
    let countdown = TextLine {
        text: &countdown_text,
        size: countdown_size,
        weight: FW_BOLD as i32,
        color: rgb(112, 198, 255),
        top: height * 53 / 100,
    };
    draw_line(memory_dc, &countdown, width);

    let hint_text = crate::i18n::overlay_hint(lang, EMERGENCY_ENABLED.load(Ordering::SeqCst));
    let hint = TextLine {
        text: hint_text,
        size: hint_size,
        weight: 400,
        color: rgb(150, 162, 174),
        top: height * 70 / 100,
    };
    draw_line(memory_dc, &hint, width);

    BitBlt(hdc, 0, 0, width, height, memory_dc, 0, 0, SRCCOPY);

    SelectObject(memory_dc, old_bitmap);
    DeleteObject(bitmap);
    DeleteDC(memory_dc);
    EndPaint(hwnd, &paint_struct);
}

unsafe fn fill_background(hdc: HDC, rect: &RECT) {
    let brush = CreateSolidBrush(rgb(20, 24, 28));
    if !brush.is_null() {
        FillRect(hdc, rect, brush);
        DeleteObject(brush);
    }
}

unsafe fn draw_line(hdc: HDC, line: &TextLine, width: i32) {
    if line.size <= 0 || width <= 0 {
        return;
    }
    let face = wide("Microsoft YaHei UI");
    let font = CreateFontW(
        line.size,
        0,
        0,
        0,
        line.weight,
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
    SetTextColor(hdc, line.color);
    SetBkMode(hdc, TRANSPARENT as i32);

    let line_height = (line.size * 3 / 2).max(line.size);
    let mut rect = RECT {
        left: 0,
        top: line.top,
        right: width,
        bottom: line.top.saturating_add(line_height),
    };
    let text = wide(line.text);
    DrawTextW(
        hdc,
        text.as_ptr(),
        -1,
        &mut rect,
        DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
    );

    SelectObject(hdc, old_font);
    DeleteObject(font);
}

const fn rgb(red: u8, green: u8, blue: u8) -> u32 {
    (red as u32) | ((green as u32) << 8) | ((blue as u32) << 16)
}
