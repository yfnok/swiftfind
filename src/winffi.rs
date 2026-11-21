//! Minimal hand-written Win32 bindings (x64/arm64). Keeping this tiny avoids a heavy
//! dependency and keeps the binary small. Only compiled on Windows.
#![cfg(windows)]
#![allow(non_snake_case, dead_code, clippy::upper_case_acronyms)]

use std::ffi::c_void;

pub type HWND = isize;
pub type WPARAM = usize;
pub type LPARAM = isize;
pub type LRESULT = isize;

pub const WM_COPYDATA: u32 = 0x004A;
pub const WM_HOTKEY: u32 = 0x0312;
pub const WM_USER: u32 = 0x0400;
pub const WM_APP: u32 = 0x8000;
pub const WM_QUIT: u32 = 0x0012;
pub const WM_SETTEXT: u32 = 0x000C;
pub const WM_GETTEXT: u32 = 0x000D;
pub const WM_GETTEXTLENGTH: u32 = 0x000E;
pub const BM_CLICK: u32 = 0x00F5;

pub const MOD_ALT: u32 = 0x1;
pub const MOD_CONTROL: u32 = 0x2;
pub const MOD_SHIFT: u32 = 0x4;
pub const MOD_WIN: u32 = 0x8;
pub const MOD_NOREPEAT: u32 = 0x4000;

pub const SW_HIDE: i32 = 0;
pub const SW_SHOW: i32 = 5;
pub const SW_SHOWNORMAL: i32 = 1;

pub const SWP_NOSIZE: u32 = 0x0001;
pub const SWP_SHOWWINDOW: u32 = 0x0040;
pub const HWND_TOPMOST: HWND = -1;

pub const PM_REMOVE: u32 = 1;
pub const SMTO_ABORTIFHUNG: u32 = 0x0002;
pub const MONITOR_DEFAULTTONEAREST: u32 = 2;

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct POINT {
    pub x: i32,
    pub y: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct RECT {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

#[repr(C)]
#[derive(Default)]
pub struct SYSTEMTIME {
    pub wYear: u16,
    pub wMonth: u16,
    pub wDayOfWeek: u16,
    pub wDay: u16,
    pub wHour: u16,
    pub wMinute: u16,
    pub wSecond: u16,
    pub wMilliseconds: u16,
}

#[repr(C)]
pub struct SHFILEINFOW {
    pub hIcon: isize,
    pub iIcon: i32,
    pub dwAttributes: u32,
    pub szDisplayName: [u16; 260],
    pub szTypeName: [u16; 80],
}

#[repr(C)]
pub struct ICONINFO {
    pub fIcon: i32,
    pub xHotspot: u32,
    pub yHotspot: u32,
    pub hbmMask: isize,
    pub hbmColor: isize,
}

#[repr(C)]
pub struct BITMAP {
    pub bmType: i32,
    pub bmWidth: i32,
    pub bmHeight: i32,
    pub bmWidthBytes: i32,
    pub bmPlanes: u16,
    pub bmBitsPixel: u16,
    pub bmBits: *mut c_void,
}

#[repr(C)]
pub struct BITMAPINFOHEADER {
    pub biSize: u32,
    pub biWidth: i32,
    pub biHeight: i32,
    pub biPlanes: u16,
    pub biBitCount: u16,
    pub biCompression: u32,
    pub biSizeImage: u32,
    pub biXPelsPerMeter: i32,
    pub biYPelsPerMeter: i32,
    pub biClrUsed: u32,
    pub biClrImportant: u32,
}

#[repr(C)]
pub struct BITMAPINFO {
    pub bmiHeader: BITMAPINFOHEADER,
    pub bmiColors: [u32; 1],
}

#[repr(C)]
pub struct MSG {
    pub hwnd: HWND,
    pub message: u32,
    pub wparam: WPARAM,
    pub lparam: LPARAM,
    pub time: u32,
    pub pt: POINT,
}

impl Default for MSG {
    fn default() -> Self {
        Self {
            hwnd: 0,
            message: 0,
            wparam: 0,
            lparam: 0,
            time: 0,
            pt: POINT::default(),
        }
    }
}

pub type WndProc = unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT;

#[repr(C)]
pub struct WNDCLASSW {
    pub style: u32,
    pub lpfnWndProc: Option<WndProc>,
    pub cbClsExtra: i32,
    pub cbWndExtra: i32,
    pub hInstance: isize,
    pub hIcon: isize,
    pub hCursor: isize,
    pub hbrBackground: isize,
    pub lpszMenuName: *const u16,
    pub lpszClassName: *const u16,
}

#[repr(C)]
pub struct COPYDATASTRUCT {
    pub dwData: usize,
    pub cbData: u32,
    pub lpData: *mut c_void,
}

#[repr(C)]
pub struct MONITORINFO {
    pub cbSize: u32,
    pub rcMonitor: RECT,
    pub rcWork: RECT,
    pub dwFlags: u32,
}

#[link(name = "user32")]
extern "system" {
    pub fn GetIconInfo(icon: isize, info: *mut ICONINFO) -> i32;
    pub fn DestroyIcon(icon: isize) -> i32;
    pub fn GetDC(hwnd: HWND) -> isize;
    pub fn ReleaseDC(hwnd: HWND, hdc: isize) -> i32;
    pub fn MessageBoxW(h: HWND, text: *const u16, caption: *const u16, flags: u32) -> i32;
    pub fn RegisterClassW(wc: *const WNDCLASSW) -> u16;
    pub fn CreateWindowExW(
        ex_style: u32,
        class_name: *const u16,
        window_name: *const u16,
        style: u32,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        parent: HWND,
        menu: isize,
        instance: isize,
        param: *mut c_void,
    ) -> HWND;
    pub fn DefWindowProcW(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT;
    pub fn DestroyWindow(h: HWND) -> i32;
    pub fn SendMessageTimeoutW(
        h: HWND,
        msg: u32,
        w: WPARAM,
        l: LPARAM,
        flags: u32,
        timeout_ms: u32,
        result: *mut usize,
    ) -> LRESULT;
    pub fn SendMessageW(h: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT;
    pub fn PeekMessageW(msg: *mut MSG, h: HWND, min: u32, max: u32, remove: u32) -> i32;
    pub fn GetMessageW(msg: *mut MSG, h: HWND, min: u32, max: u32) -> i32;
    pub fn TranslateMessage(msg: *const MSG) -> i32;
    pub fn DispatchMessageW(msg: *const MSG) -> LRESULT;
    pub fn PostThreadMessageW(thread_id: u32, msg: u32, w: WPARAM, l: LPARAM) -> i32;
    pub fn FindWindowW(class_name: *const u16, window_name: *const u16) -> HWND;
    pub fn RegisterHotKey(h: HWND, id: i32, modifiers: u32, vk: u32) -> i32;
    pub fn UnregisterHotKey(h: HWND, id: i32) -> i32;
    pub fn ShowWindow(h: HWND, cmd: i32) -> i32;
    pub fn IsWindowVisible(h: HWND) -> i32;
    pub fn IsWindow(h: HWND) -> i32;
    pub fn GetClassNameW(h: HWND, buf: *mut u16, max: i32) -> i32;
    pub fn GetDlgItem(h: HWND, id: i32) -> HWND;
    pub fn FindWindowExW(parent: HWND, after: HWND, class: *const u16, title: *const u16) -> HWND;
    pub fn SetForegroundWindow(h: HWND) -> i32;
    pub fn GetForegroundWindow() -> HWND;
    pub fn BringWindowToTop(h: HWND) -> i32;
    pub fn GetWindowThreadProcessId(h: HWND, pid: *mut u32) -> u32;
    pub fn AttachThreadInput(attach: u32, attach_to: u32, f_attach: i32) -> i32;
    pub fn SetWindowPos(h: HWND, after: HWND, x: i32, y: i32, cx: i32, cy: i32, flags: u32) -> i32;
    pub fn GetWindowRect(h: HWND, rect: *mut RECT) -> i32;
    pub fn GetCursorPos(p: *mut POINT) -> i32;
    pub fn MonitorFromPoint(p: POINT, flags: u32) -> isize;
    pub fn GetMonitorInfoW(m: isize, info: *mut MONITORINFO) -> i32;
    pub fn OpenClipboard(owner: HWND) -> i32;
    pub fn CloseClipboard() -> i32;
    pub fn EmptyClipboard() -> i32;
    pub fn GetClipboardData(format: u32) -> isize;
    pub fn SetClipboardData(format: u32, mem: isize) -> isize;
    pub fn IsClipboardFormatAvailable(format: u32) -> i32;
    pub fn GetClipboardSequenceNumber() -> u32;
    pub fn RegisterClipboardFormatW(name: *const u16) -> u32;
    pub fn keybd_event(vk: u8, scan: u8, flags: u32, extra: usize);
}

pub const KEYEVENTF_KEYUP: u32 = 0x0002;
pub const VK_CONTROL: u8 = 0x11;
pub const VK_V: u8 = 0x56;
pub const GMEM_MOVEABLE: u32 = 0x0002;
pub const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;

#[link(name = "gdi32")]
extern "system" {
    pub fn GetObjectW(handle: isize, cb: i32, out: *mut c_void) -> i32;
    pub fn GetDIBits(hdc: isize, bitmap: isize, start: u32, lines: u32, bits: *mut c_void, bmi: *mut BITMAPINFO, usage: u32) -> i32;
    pub fn DeleteObject(obj: isize) -> i32;
}

#[link(name = "ole32")]
extern "system" {
    pub fn CoInitializeEx(reserved: *mut c_void, coinit: u32) -> i32;
}

#[link(name = "kernel32")]
extern "system" {
    pub fn GetLocalTime(t: *mut SYSTEMTIME);
    pub fn GetModuleHandleW(name: *const u16) -> isize;
    pub fn GetCurrentThreadId() -> u32;
    pub fn SetProcessWorkingSetSize(process: isize, min: usize, max: usize) -> i32;
    pub fn GetCurrentProcess() -> isize;
    pub fn CreateMutexW(attrs: *const c_void, initial_owner: i32, name: *const u16) -> isize;
    pub fn GetLastError() -> u32;
    pub fn GlobalAlloc(flags: u32, bytes: usize) -> isize;
    pub fn GlobalFree(mem: isize) -> isize;
    pub fn GlobalLock(mem: isize) -> *mut c_void;
    pub fn GlobalUnlock(mem: isize) -> i32;
    pub fn GlobalSize(mem: isize) -> usize;
    pub fn OpenProcess(access: u32, inherit: i32, pid: u32) -> isize;
    pub fn CloseHandle(h: isize) -> i32;
    pub fn QueryFullProcessImageNameW(
        process: isize,
        flags: u32,
        name: *mut u16,
        size: *mut u32,
    ) -> i32;
}

pub const ERROR_ALREADY_EXISTS: u32 = 183;

#[link(name = "shell32")]
extern "system" {
    pub fn SHGetFileInfoW(path: *const u16, attrs: u32, info: *mut SHFILEINFOW, cb: u32, flags: u32) -> usize;
    pub fn ShellExecuteW(
        h: HWND,
        verb: *const u16,
        file: *const u16,
        params: *const u16,
        dir: *const u16,
        show: i32,
    ) -> isize;
}

/// UTF-8 → null-terminated UTF-16.
pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}
