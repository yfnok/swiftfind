//! Platform layer: launching things, global hotkey, tray icon, window show/hide.
//! Everything Windows-specific lives here (plus `winffi` and the Everything IPC window);
//! on other OSes the functions are harmless stubs so the rest of the crate builds and tests.

use crate::clipdb::ClipContent;
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

pub const WINDOW_TITLE: &str = "SwiftFind";

/// State shared between the UI thread and the hotkey/tray thread.
pub struct Shared {
    /// The launcher window is currently on screen.
    pub visible: AtomicBool,
    /// Set by the hotkey thread when the window was just shown; the UI resets itself and focuses.
    pub just_shown: AtomicBool,
    /// Set when the settings panel should open on the next frame.
    pub open_panel: AtomicBool,
    /// Cached HWND of our main window.
    pub hwnd: AtomicIsize,
    /// Thread id of the hotkey thread (for re-registering the hotkey).
    pub hotkey_tid: AtomicU32,
    /// Launcher hotkey string to (re)register.
    pub hotkey: Mutex<String>,
    /// Clipboard-history hotkey (empty = disabled).
    pub clip_hotkey: Mutex<String>,
    /// Last registration error for each hotkey, shown in the settings panel.
    pub hotkey_error: Mutex<Option<String>>,
    pub clip_hotkey_error: Mutex<Option<String>>,
    /// While the user is recording a new hotkey, global hotkeys are released so that pressing the
    /// current combination is captured by the recorder instead of toggling the window.
    pub suspended: AtomicBool,
    /// Query to pre-fill the next time the window is shown (set by the clipboard hotkey).
    pub pending_query: Mutex<Option<String>>,
}

impl Shared {
    pub fn new(hotkey: &str, clip_hotkey: &str, start_visible: bool) -> Arc<Self> {
        Arc::new(Self {
            visible: AtomicBool::new(start_visible),
            just_shown: AtomicBool::new(false),
            open_panel: AtomicBool::new(false),
            hwnd: AtomicIsize::new(0),
            hotkey_tid: AtomicU32::new(0),
            hotkey: Mutex::new(hotkey.to_string()),
            clip_hotkey: Mutex::new(clip_hotkey.to_string()),
            hotkey_error: Mutex::new(None),
            clip_hotkey_error: Mutex::new(None),
            suspended: AtomicBool::new(false),
            pending_query: Mutex::new(None),
        })
    }
}

// ───────────────────────── pure helpers (tested everywhere) ─────────────────────────

pub const MOD_ALT: u32 = 0x1;
pub const MOD_CONTROL: u32 = 0x2;
pub const MOD_SHIFT: u32 = 0x4;
pub const MOD_WIN: u32 = 0x8;

/// Parse `Ctrl+Alt+F`, `Alt+Space`, `Win+Shift+F12` … into (modifiers, virtual-key).
pub fn parse_hotkey(s: &str) -> Result<(u32, u32), String> {
    let mut mods = 0u32;
    let mut vk: Option<u32> = None;
    for part in s.split('+').map(|p| p.trim()).filter(|p| !p.is_empty()) {
        let up = part.to_ascii_uppercase();
        match up.as_str() {
            "ALT" => mods |= MOD_ALT,
            "CTRL" | "CONTROL" => mods |= MOD_CONTROL,
            "SHIFT" => mods |= MOD_SHIFT,
            "WIN" | "WINDOWS" | "SUPER" | "META" => mods |= MOD_WIN,
            "SPACE" => vk = Some(0x20),
            "ENTER" | "RETURN" => vk = Some(0x0D),
            "TAB" => vk = Some(0x09),
            "ESC" | "ESCAPE" => vk = Some(0x1B),
            "BACKSPACE" => vk = Some(0x08),
            "`" | "~" | "GRAVE" | "BACKTICK" => vk = Some(0xC0),
            "," | "COMMA" => vk = Some(0xBC),
            "." | "PERIOD" => vk = Some(0xBE),
            "/" | "SLASH" => vk = Some(0xBF),
            ";" | "SEMICOLON" => vk = Some(0xBA),
            "-" | "MINUS" => vk = Some(0xBD),
            "=" | "EQUALS" | "PLUS" => vk = Some(0xBB),
            "[" | "OPENBRACKET" => vk = Some(0xDB),
            "]" | "CLOSEBRACKET" => vk = Some(0xDD),
            "\\" | "BACKSLASH" => vk = Some(0xDC),
            "'" | "QUOTE" => vk = Some(0xDE),
            "INSERT" => vk = Some(0x2D),
            "DELETE" => vk = Some(0x2E),
            "HOME" => vk = Some(0x24),
            "END" => vk = Some(0x23),
            "PAGEUP" => vk = Some(0x21),
            "PAGEDOWN" => vk = Some(0x22),
            "ARROWLEFT" | "LEFT" => vk = Some(0x25),
            "ARROWUP" | "UP" => vk = Some(0x26),
            "ARROWRIGHT" | "RIGHT" => vk = Some(0x27),
            "ARROWDOWN" | "DOWN" => vk = Some(0x28),
            _ => {
                let b = up.as_bytes();
                if b.len() == 1 && (b[0].is_ascii_uppercase() || b[0].is_ascii_digit()) {
                    vk = Some(b[0] as u32);
                } else if up.starts_with('F') && up.len() <= 3 {
                    match up[1..].parse::<u32>() {
                        Ok(n) if (1..=24).contains(&n) => vk = Some(0x70 + n - 1),
                        _ => return Err(format!("无法识别的按键: {part}")),
                    }
                } else {
                    return Err(format!("无法识别的按键: {part}"));
                }
            }
        }
    }
    match vk {
        Some(v) if mods != 0 => Ok((mods, v)),
        Some(_) => Err("快捷键至少需要一个修饰键 (Alt / Ctrl / Shift / Win)".into()),
        None => Err("快捷键缺少主键".into()),
    }
}

/// Build a canonical hotkey string from recorded modifiers and a key name (egui `Key::name()`).
/// Returns `None` when the result would not be a valid global hotkey.
pub fn compose_hotkey(ctrl: bool, alt: bool, shift: bool, win: bool, key: &str) -> Option<String> {
    if !(ctrl || alt || shift || win) {
        return None;
    }
    let mut parts: Vec<&str> = Vec::new();
    if ctrl {
        parts.push("Ctrl");
    }
    if alt {
        parts.push("Alt");
    }
    if shift {
        parts.push("Shift");
    }
    if win {
        parts.push("Win");
    }
    parts.push(key);
    let s = parts.join("+");
    parse_hotkey(&s).ok().map(|_| s)
}

/// Expand `%VAR%` references using the process environment.
pub fn expand_env(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(start) = rest.find('%') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match after.find('%') {
            Some(end) if end > 0 => match std::env::var(&after[..end]) {
                Ok(v) => {
                    out.push_str(&v);
                    rest = &after[end + 1..];
                }
                Err(_) => {
                    out.push('%');
                    rest = after;
                }
            },
            _ => {
                out.push('%');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

// ───────────────────────────── Windows implementation ─────────────────────────────

#[cfg(windows)]
mod imp {
    use super::*;
    use crate::winffi::*;
    use std::os::windows::process::CommandExt;

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    pub fn shell_open(
        target: &str,
        args: &str,
        admin: bool,
        cwd: Option<&str>,
    ) -> Result<(), String> {
        let target = expand_env(target);
        let args = expand_env(args);
        let is_uri = target.contains(':')
            && !target.contains('\\')
            && !target.contains('/')
            && target.len() > 2;
        let verb = wide(if admin && !is_uri { "runas" } else { "open" });
        let file = wide(&target);
        let params = wide(&args);
        let dir = cwd.map(|d| wide(&expand_env(d)));
        let r = unsafe {
            ShellExecuteW(
                0,
                verb.as_ptr(),
                file.as_ptr(),
                if args.is_empty() {
                    std::ptr::null()
                } else {
                    params.as_ptr()
                },
                dir.as_ref().map(|d| d.as_ptr()).unwrap_or(std::ptr::null()),
                SW_SHOWNORMAL,
            )
        };
        if r > 32 {
            Ok(())
        } else if r == 5 && admin {
            Err("已取消管理员授权".into())
        } else {
            Err(format!("无法启动 {target} (错误码 {r})"))
        }
    }

    pub fn reveal_in_explorer(path: &str) {
        let _ = std::process::Command::new("explorer.exe")
            .raw_arg(format!("/select,\"{}\"", path))
            .spawn();
    }

    unsafe fn alloc_global(bytes: &[u8]) -> isize {
        let h = GlobalAlloc(GMEM_MOVEABLE, bytes.len().max(1));
        if h == 0 {
            return 0;
        }
        let p = GlobalLock(h) as *mut u8;
        if p.is_null() {
            GlobalFree(h);
            return 0;
        }
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), p, bytes.len());
        GlobalUnlock(h);
        h
    }

    /// Put a history item back on the clipboard. Text goes out as CF_UNICODETEXT; images as both
    /// CF_DIB (works everywhere) and the registered "PNG" format (keeps transparency).
    pub fn set_clip(c: &ClipContent) -> bool {
        unsafe {
            let mut items: Vec<(u32, isize)> = Vec::new();
            match c {
                ClipContent::Text(t) => {
                    let bytes: Vec<u8> = t
                        .encode_utf16()
                        .chain(std::iter::once(0))
                        .flat_map(|u| u.to_le_bytes())
                        .collect();
                    items.push((13, alloc_global(&bytes)));
                }
                ClipContent::Png(png) => {
                    let Some(img) = crate::clipimg::decode_png(png) else {
                        return false;
                    };
                    items.push((8, alloc_global(&crate::clipimg::rgba_to_dib(&img))));
                    let fmt = RegisterClipboardFormatW(wide("PNG").as_ptr());
                    if fmt != 0 {
                        items.push((fmt, alloc_global(png)));
                    }
                }
            }
            let free_all = |items: &[(u32, isize)]| {
                for (_, h) in items {
                    if *h != 0 {
                        GlobalFree(*h);
                    }
                }
            };
            if items.iter().any(|(_, h)| *h == 0) {
                free_all(&items);
                return false;
            }
            let mut opened = false;
            for _ in 0..10 {
                if OpenClipboard(0) != 0 {
                    opened = true;
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            if !opened {
                free_all(&items);
                return false;
            }
            EmptyClipboard();
            let mut ok = false;
            for (fmt, h) in items {
                // On success the clipboard owns the memory.
                if SetClipboardData(fmt, h) != 0 {
                    ok = true;
                } else {
                    GlobalFree(h);
                }
            }
            CloseClipboard();
            ok
        }
    }

    /// Put `c` on the clipboard and send Ctrl+V to whichever window gets focus once the
    /// launcher is hidden. Call this *before* hiding the window.
    pub fn paste_clip(c: &ClipContent) -> bool {
        if !set_clip(c) {
            return false;
        }
        std::thread::spawn(|| {
            std::thread::sleep(std::time::Duration::from_millis(160));
            unsafe {
                keybd_event(VK_CONTROL, 0, 0, 0);
                keybd_event(VK_V, 0, 0, 0);
                keybd_event(VK_V, 0, KEYEVENTF_KEYUP, 0);
                keybd_event(VK_CONTROL, 0, KEYEVENTF_KEYUP, 0);
            }
        });
        true
    }

    pub fn find_main_hwnd(shared: &Shared) -> HWND {
        let cached = shared.hwnd.load(Ordering::Relaxed);
        if cached != 0 {
            return cached;
        }
        let title = wide(WINDOW_TITLE);
        let h = unsafe { FindWindowW(std::ptr::null(), title.as_ptr()) };
        if h != 0 {
            shared.hwnd.store(h, Ordering::Relaxed);
        }
        h
    }

    fn force_foreground(h: HWND) {
        unsafe {
            let fg = GetForegroundWindow();
            let me = GetCurrentThreadId();
            let fg_tid = if fg != 0 {
                GetWindowThreadProcessId(fg, std::ptr::null_mut())
            } else {
                0
            };
            if fg_tid != 0 && fg_tid != me {
                AttachThreadInput(me, fg_tid, 1);
                BringWindowToTop(h);
                SetForegroundWindow(h);
                AttachThreadInput(me, fg_tid, 0);
            } else {
                SetForegroundWindow(h);
            }
        }
    }

    /// Show the launcher centered (upper third) on the monitor that holds the mouse cursor.
    pub fn show_window(shared: &Shared) {
        let h = find_main_hwnd(shared);
        if h == 0 {
            return;
        }
        unsafe {
            let mut cur = POINT::default();
            GetCursorPos(&mut cur);
            let mon = MonitorFromPoint(cur, MONITOR_DEFAULTTONEAREST);
            let mut mi = MONITORINFO {
                cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                ..std::mem::zeroed()
            };
            let mut rc = RECT::default();
            GetWindowRect(h, &mut rc);
            let (w, _) = (rc.right - rc.left, rc.bottom - rc.top);
            if GetMonitorInfoW(mon, &mut mi) != 0 {
                let work = mi.rcWork;
                let x = work.left + ((work.right - work.left) - w) / 2;
                let y = work.top + (work.bottom - work.top) * 15 / 100;
                SetWindowPos(h, HWND_TOPMOST, x, y, 0, 0, SWP_NOSIZE | SWP_SHOWWINDOW);
            }
            ShowWindow(h, SW_SHOW);
            force_foreground(h);
        }
        shared.visible.store(true, Ordering::SeqCst);
        shared.just_shown.store(true, Ordering::SeqCst);
    }

    pub fn hide_window(shared: &Shared) {
        let h = find_main_hwnd(shared);
        if h != 0 {
            unsafe {
                ShowWindow(h, SW_HIDE);
            }
        }
        shared.visible.store(false, Ordering::SeqCst);
        // Give memory back to the OS while we sit in the tray.
        std::thread::spawn(|| {
            std::thread::sleep(std::time::Duration::from_millis(1500));
            unsafe {
                SetProcessWorkingSetSize(GetCurrentProcess(), usize::MAX, usize::MAX);
            }
        });
    }

    pub fn toggle_window(shared: &Shared) {
        // Trust the real window state: a second launch can show the window behind our back.
        let h = find_main_hwnd(shared);
        let visible = if h != 0 { unsafe { IsWindowVisible(h) != 0 } } else { shared.visible.load(Ordering::SeqCst) };
        if visible {
            hide_window(shared);
        } else {
            show_window(shared);
        }
    }

    /// Modal error dialog (the release build has no console to print to).
    pub fn error_box(text: &str) {
        unsafe {
            MessageBoxW(0, wide(text).as_ptr(), wide("SwiftFind").as_ptr(), 0x10);
        }
    }
    /// Returns false if another instance already owns the mutex (and wakes that instance).
    pub fn acquire_single_instance() -> bool {
        let name = wide("Local\\SwiftFind.SingleInstance");
        unsafe {
            CreateMutexW(std::ptr::null(), 1, name.as_ptr());
            if GetLastError() == ERROR_ALREADY_EXISTS {
                let title = wide(WINDOW_TITLE);
                let h = FindWindowW(std::ptr::null(), title.as_ptr());
                if h != 0 {
                    ShowWindow(h, SW_SHOW);
                    force_foreground(h);
                }
                return false;
            }
        }
        true
    }

    fn register_one(id: i32, hotkey: &str) -> Result<(), String> {
        unsafe {
            UnregisterHotKey(0, id);
        }
        if hotkey.trim().is_empty() {
            return Ok(());
        }
        let (mods, vk) = parse_hotkey(hotkey)?;
        unsafe {
            if RegisterHotKey(0, id, mods | MOD_NOREPEAT, vk) == 0 {
                return Err(format!("快捷键 {hotkey} 已被其他程序占用，请换一个"));
            }
        }
        Ok(())
    }

    fn apply_hotkey(shared: &Shared) {
        let hk = shared.hotkey.lock().map(|h| h.clone()).unwrap_or_default();
        let ck = shared
            .clip_hotkey
            .lock()
            .map(|h| h.clone())
            .unwrap_or_default();
        let (r1, r2) = if shared.suspended.load(Ordering::SeqCst) {
            unsafe {
                UnregisterHotKey(0, 1);
                UnregisterHotKey(0, 2);
            }
            (Ok(()), Ok(()))
        } else {
            let r1 = register_one(1, &hk);
            let r2 = if !ck.trim().is_empty() && ck.trim().eq_ignore_ascii_case(hk.trim()) {
                unsafe {
                    UnregisterHotKey(0, 2);
                }
                Err("与主快捷键相同".to_string())
            } else {
                register_one(2, &ck)
            };
            (r1, r2)
        };
        if let Ok(mut e) = shared.hotkey_error.lock() {
            *e = r1.err();
        }
        if let Ok(mut e) = shared.clip_hotkey_error.lock() {
            *e = r2.err();
        }
    }

    /// Ask the hotkey thread to re-read `shared.hotkey` and register it.
    pub fn reapply_hotkey(shared: &Shared) {
        let tid = shared.hotkey_tid.load(Ordering::SeqCst);
        if tid != 0 {
            unsafe {
                PostThreadMessageW(tid, WM_APP + 1, 0, 0);
            }
        }
    }

    pub fn spawn_hotkey_and_tray(shared: Arc<Shared>, wake: Arc<dyn Fn() + Send + Sync>) {
        std::thread::Builder::new()
            .name("hotkey-tray".into())
            .spawn(move || unsafe {
                shared
                    .hotkey_tid
                    .store(GetCurrentThreadId(), Ordering::SeqCst);
                apply_hotkey(&shared);
                // Tell the UI about a failed startup registration.
                wake();

                // ── tray icon (lives on this thread, pumped by the loop below) ──
                use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
                use tray_icon::{
                    Icon, MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent,
                };

                let menu = Menu::new();
                let m_show = MenuItem::new("显示 / 隐藏", true, None);
                let m_settings = MenuItem::new("设置", true, None);
                let m_quit = MenuItem::new("退出", true, None);
                let _ = menu.append(&m_show);
                let _ = menu.append(&m_settings);
                let _ = menu.append(&PredefinedMenuItem::separator());
                let _ = menu.append(&m_quit);
                let (show_id, settings_id, quit_id) = (
                    m_show.id().clone(),
                    m_settings.id().clone(),
                    m_quit.id().clone(),
                );

                let sh = shared.clone();
                let wk = wake.clone();
                MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
                    if e.id == show_id {
                        toggle_window(&sh);
                    } else if e.id == settings_id {
                        sh.open_panel.store(true, Ordering::SeqCst);
                        show_window(&sh);
                    } else if e.id == quit_id {
                        std::process::exit(0);
                    }
                    wk();
                }));
                let sh = shared.clone();
                let wk = wake.clone();
                TrayIconEvent::set_event_handler(Some(move |e: TrayIconEvent| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = e
                    {
                        toggle_window(&sh);
                        wk();
                    }
                }));
                let _tray = Icon::from_rgba(crate::icon::rgba(32), 32, 32)
                    .ok()
                    .and_then(|icon| {
                        TrayIconBuilder::new()
                            .with_menu(Box::new(menu))
                            .with_tooltip("SwiftFind")
                            .with_icon(icon)
                            .build()
                            .ok()
                    });

                let mut msg = MSG::default();
                while GetMessageW(&mut msg, 0, 0, 0) > 0 {
                    match msg.message {
                        WM_HOTKEY => {
                            crate::log::guarded("hotkey handler", || {
                                if msg.wparam == 2 {
                                    // Clipboard hotkey: always open with the clipboard view.
                                    if let Ok(mut q) = shared.pending_query.lock() {
                                        *q = Some("cb ".to_string());
                                    }
                                    show_window(&shared);
                                } else {
                                    toggle_window(&shared);
                                }
                                wake();
                            });
                        }
                        m if m == WM_APP + 1 => {
                            crate::log::guarded("hotkey re-register", || {
                                apply_hotkey(&shared);
                                wake();
                            });
                        }
                        _ => {
                            TranslateMessage(&msg);
                            DispatchMessageW(&msg);
                        }
                    }
                }
            })
            .expect("spawn hotkey thread");
    }

    pub fn set_autostart(enable: bool) {
        let exe = std::env::current_exe()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        let key = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
        let mut cmd = std::process::Command::new("reg.exe");
        if enable {
            let value = format!("\"{exe}\" --background");
            cmd.args([
                "add",
                key,
                "/v",
                "SwiftFind",
                "/t",
                "REG_SZ",
                "/d",
                value.as_str(),
                "/f",
            ]);
        } else {
            cmd.args(["delete", key, "/v", "SwiftFind", "/f"]);
        }
        let _ = cmd.creation_flags(CREATE_NO_WINDOW).output();
    }

    pub fn autostart_enabled() -> bool {
        std::process::Command::new("reg.exe")
            .args([
                "query",
                r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
                "/v",
                "SwiftFind",
            ])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
}

// ───────────────────────────── non-Windows stubs ─────────────────────────────

#[cfg(not(windows))]
mod imp {
    use super::*;

    pub fn shell_open(
        target: &str,
        _args: &str,
        _admin: bool,
        _cwd: Option<&str>,
    ) -> Result<(), String> {
        Err(format!("仅支持 Windows (无法打开 {target})"))
    }
    pub fn reveal_in_explorer(_path: &str) {}
    pub fn set_clip(_c: &ClipContent) -> bool {
        false
    }
    pub fn paste_clip(_c: &ClipContent) -> bool {
        false
    }
    pub fn show_window(shared: &Shared) {
        shared.visible.store(true, Ordering::SeqCst);
        shared.just_shown.store(true, Ordering::SeqCst);
    }
    pub fn hide_window(shared: &Shared) {
        shared.visible.store(false, Ordering::SeqCst);
    }
    pub fn toggle_window(shared: &Shared) {
        if shared.visible.load(Ordering::SeqCst) {
            hide_window(shared)
        } else {
            show_window(shared)
        }
    }
    pub fn error_box(text: &str) {
        eprintln!("{text}");
    }
    pub fn acquire_single_instance() -> bool {
        true
    }
    pub fn reapply_hotkey(_shared: &Shared) {}
    pub fn spawn_hotkey_and_tray(_shared: Arc<Shared>, _wake: Arc<dyn Fn() + Send + Sync>) {}
    pub fn set_autostart(_enable: bool) {}
    pub fn autostart_enabled() -> bool {
        false
    }
}

pub use imp::*;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hotkeys_parse() {
        assert_eq!(parse_hotkey("Alt+Space").unwrap(), (MOD_ALT, 0x20));
        assert_eq!(
            parse_hotkey("ctrl + alt + F").unwrap(),
            (MOD_CONTROL | MOD_ALT, b'F' as u32)
        );
        assert_eq!(parse_hotkey("Win+F12").unwrap(), (MOD_WIN, 0x7B));
        assert!(parse_hotkey("Space").is_err());
        assert!(parse_hotkey("Alt+").is_err());
        assert!(parse_hotkey("Alt+Foo").is_err());
    }

    #[test]
    fn compose_roundtrip() {
        let h = compose_hotkey(true, true, false, false, "K").unwrap();
        assert_eq!(h, "Ctrl+Alt+K");
        assert_eq!(
            parse_hotkey(&h).unwrap(),
            (MOD_CONTROL | MOD_ALT, b'K' as u32)
        );
        assert_eq!(
            compose_hotkey(false, true, false, false, "Space").as_deref(),
            Some("Alt+Space")
        );
        assert_eq!(
            compose_hotkey(false, false, true, true, "F9").as_deref(),
            Some("Shift+Win+F9")
        );
        assert!(compose_hotkey(false, false, false, false, "A").is_none()); // no modifier
        assert!(compose_hotkey(true, false, false, false, "Bogus").is_none());
        for k in [
            "Backtick",
            "Comma",
            "Period",
            "Slash",
            "Semicolon",
            "Minus",
            "Equals",
            "ArrowUp",
            "PageDown",
            "Delete",
        ] {
            assert!(
                compose_hotkey(true, false, false, false, k).is_some(),
                "{k}"
            );
        }
    }

    #[test]
    fn env_expansion() {
        std::env::set_var("SF_TEST_VAR", "xyz");
        assert_eq!(expand_env("%SF_TEST_VAR%\\a"), "xyz\\a");
        assert_eq!(expand_env("100%"), "100%");
        assert_eq!(expand_env("%NOPE_SF%x"), "%NOPE_SF%x");
    }
}
