//! Clipboard watcher: records text and images into the redb history (`clipdb`).
//!
//! The watcher polls `GetClipboardSequenceNumber` (a single cheap call) and only reads the
//! clipboard when it changed. It runs on its own thread with its own database connection.
//! Text wins over images when both are present (Excel puts both on the clipboard).

use crate::clipdb::Store;
use crate::config::Config;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

pub fn db_path() -> PathBuf {
    crate::config::data_dir().join("clipboard.redb")
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Live settings shared with the watcher thread (the settings panel updates them instantly).
pub struct ClipSettings {
    pub enabled: AtomicBool,
    pub images: AtomicBool,
    pub max_items: AtomicUsize,
    pub max_days: AtomicU32,
    pub image_mb: AtomicU32,
    pub ignore: Mutex<Vec<String>>,
}

impl ClipSettings {
    pub fn from_config(cfg: &Config) -> Arc<Self> {
        let s = Arc::new(Self {
            enabled: AtomicBool::new(true),
            images: AtomicBool::new(true),
            max_items: AtomicUsize::new(2000),
            max_days: AtomicU32::new(90),
            image_mb: AtomicU32::new(300),
            ignore: Mutex::new(Vec::new()),
        });
        s.apply(cfg);
        s
    }

    pub fn apply(&self, cfg: &Config) {
        self.enabled.store(cfg.clipboard_enabled, Ordering::Relaxed);
        self.images.store(cfg.clipboard_images, Ordering::Relaxed);
        self.max_items
            .store(cfg.clipboard_max.clamp(100, 50_000), Ordering::Relaxed);
        self.max_days.store(cfg.clipboard_days, Ordering::Relaxed);
        self.image_mb
            .store(cfg.clipboard_image_mb, Ordering::Relaxed);
        if let Ok(mut g) = self.ignore.lock() {
            *g = cfg
                .clipboard_ignore_apps
                .iter()
                .map(|a| normalize_app(a))
                .filter(|a| !a.is_empty())
                .collect();
        }
    }

    pub fn prune(&self, store: &Store) {
        store.prune(
            self.max_items.load(Ordering::Relaxed),
            self.max_days.load(Ordering::Relaxed),
            self.image_mb.load(Ordering::Relaxed) as u64 * 1024 * 1024,
            now_ms(),
        );
    }

    pub fn is_ignored(&self, src: &str) -> bool {
        let src = normalize_app(src);
        !src.is_empty()
            && self
                .ignore
                .lock()
                .map(|g| g.contains(&src))
                .unwrap_or(false)
    }
}

/// `"KeePass.EXE "` -> `"keepass"`: compare program names without case or extension.
pub fn normalize_app(s: &str) -> String {
    let t = s.trim().to_lowercase();
    t.strip_suffix(".exe").unwrap_or(&t).to_string()
}

/// One-line preview for list rows: first non-empty line, whitespace collapsed, truncated.
pub fn preview(text: &str, max_chars: usize) -> String {
    let line = text
        .lines()
        .map(|l| l.trim())
        .find(|l| !l.is_empty())
        .unwrap_or("");
    let collapsed: String = line.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() > max_chars {
        let cut: String = collapsed.chars().take(max_chars).collect();
        format!("{cut}…")
    } else {
        collapsed
    }
}

pub fn ago(now_ms: u64, then_ms: u64) -> String {
    let d = now_ms.saturating_sub(then_ms) / 1000;
    match d {
        0..=59 => "刚刚".into(),
        60..=3599 => format!("{} 分钟前", d / 60),
        3600..=86_399 => format!("{} 小时前", d / 3600),
        _ => format!("{} 天前", d / 86_400),
    }
}

/// Start the watcher thread. `changed` is set after every stored clip so the UI can refresh;
/// `wake` repaints the UI.
pub fn spawn_listener(
    store: Store,
    settings: Arc<ClipSettings>,
    changed: Arc<AtomicBool>,
    wake: Box<dyn Fn() + Send>,
) {
    #[cfg(windows)]
    imp::spawn(store, settings, changed, wake);
    #[cfg(not(windows))]
    {
        let _ = (store, settings, changed, wake);
    }
}

/// Turn raw clipboard bytes into a stored image: (png, thumbnail png, width, height).
/// Prefers the PNG the source program provided (keeps transparency, no re-encode).
pub fn prepare_image(
    png: Option<&[u8]>,
    dib: Option<&[u8]>,
) -> Option<(Vec<u8>, Vec<u8>, u32, u32)> {
    use crate::clipimg::*;
    let (png_bytes, rgba) = match (png, dib) {
        (Some(p), _) => (p.to_vec(), decode_png(p)?),
        (None, Some(d)) => {
            let img = dib_to_rgba(d)?;
            (encode_png(&img)?, img)
        }
        _ => return None,
    };
    let thumb = encode_png(&thumbnail(&rgba, 128))?;
    Some((png_bytes, thumb, rgba.w, rgba.h))
}

#[cfg(windows)]
mod imp {
    use super::*;
    use crate::winffi::*;
    use std::time::Duration;

    const CF_UNICODETEXT: u32 = 13;
    const CF_DIB: u32 = 8;
    const MAX_RAW_BYTES: usize = 128 * 1024 * 1024;

    enum Snapshot {
        Text(String),
        Png(Vec<u8>),
        Dib(Vec<u8>),
    }

    /// Process image name (lowercase file name) of the window that currently has focus.
    fn foreground_exe() -> String {
        unsafe {
            let h = GetForegroundWindow();
            if h == 0 {
                return String::new();
            }
            let mut pid = 0u32;
            GetWindowThreadProcessId(h, &mut pid);
            if pid == 0 {
                return String::new();
            }
            let ph = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if ph == 0 {
                return String::new();
            }
            let mut buf = [0u16; 520];
            let mut len = buf.len() as u32;
            let ok = QueryFullProcessImageNameW(ph, 0, buf.as_mut_ptr(), &mut len);
            CloseHandle(ph);
            if ok == 0 {
                return String::new();
            }
            let full = String::from_utf16_lossy(&buf[..len as usize]);
            full.rsplit(['\\', '/']).next().unwrap_or("").to_lowercase()
        }
    }

    unsafe fn read_bytes(fmt: u32) -> Option<Vec<u8>> {
        let h = GetClipboardData(fmt);
        if h == 0 {
            return None;
        }
        let size = GlobalSize(h);
        if size == 0 || size > MAX_RAW_BYTES {
            return None;
        }
        let p = GlobalLock(h) as *const u8;
        if p.is_null() {
            return None;
        }
        let v = std::slice::from_raw_parts(p, size).to_vec();
        GlobalUnlock(h);
        Some(v)
    }

    unsafe fn read_unicode() -> Option<String> {
        let h = GetClipboardData(CF_UNICODETEXT);
        if h == 0 {
            return None;
        }
        let p = GlobalLock(h) as *const u16;
        if p.is_null() {
            return None;
        }
        let max = (GlobalSize(h) / 2).min(crate::clipdb::MAX_TEXT_CHARS * 2 + 2);
        let mut len = 0usize;
        while len < max && *p.add(len) != 0 {
            len += 1;
        }
        let s = String::from_utf16_lossy(std::slice::from_raw_parts(p, len));
        GlobalUnlock(h);
        Some(s)
    }

    fn read_snapshot(exclude_fmt: u32, png_fmt: u32, want_images: bool) -> Option<Snapshot> {
        unsafe {
            // Programs set this format to ask clipboard monitors to ignore the content.
            if exclude_fmt != 0 && IsClipboardFormatAvailable(exclude_fmt) != 0 {
                return None;
            }
            let has_text = IsClipboardFormatAvailable(CF_UNICODETEXT) != 0;
            let has_png = want_images && png_fmt != 0 && IsClipboardFormatAvailable(png_fmt) != 0;
            let has_dib = want_images && IsClipboardFormatAvailable(CF_DIB) != 0;
            if !(has_text || has_png || has_dib) {
                return None;
            }
            let mut opened = false;
            for _ in 0..8 {
                if OpenClipboard(0) != 0 {
                    opened = true;
                    break;
                }
                std::thread::sleep(Duration::from_millis(15));
            }
            if !opened {
                return None;
            }
            // Closes the clipboard even if reading panics; a clipboard left open blocks every app.
            struct Opened;
            impl Drop for Opened {
                fn drop(&mut self) {
                    unsafe {
                        CloseClipboard();
                    }
                }
            }
            let _opened = Opened;
            let snap = if has_text {
                read_unicode().map(Snapshot::Text)
            } else if has_png {
                read_bytes(png_fmt)
                    .map(Snapshot::Png)
                    .or_else(|| read_bytes(CF_DIB).map(Snapshot::Dib))
            } else {
                read_bytes(CF_DIB).map(Snapshot::Dib)
            };
            snap
        }
    }

    pub fn spawn(
        store: Store,
        settings: Arc<ClipSettings>,
        changed: Arc<AtomicBool>,
        wake: Box<dyn Fn() + Send>,
    ) {
        std::thread::Builder::new()
            .name("clipboard-watch".into())
            .spawn(move || {
                settings.prune(&store);
                let own = std::env::current_exe()
                    .ok()
                    .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_lowercase()))
                    .unwrap_or_default();
                let exclude_fmt = unsafe {
                    RegisterClipboardFormatW(
                        wide("ExcludeClipboardContentFromMonitorProcessing").as_ptr(),
                    )
                };
                let png_fmt = unsafe { RegisterClipboardFormatW(wide("PNG").as_ptr()) };
                let mut last_seq = unsafe { GetClipboardSequenceNumber() };
                loop {
                    std::thread::sleep(Duration::from_millis(400));
                    let seq = unsafe { GetClipboardSequenceNumber() };
                    if seq == last_seq {
                        continue;
                    }
                    last_seq = seq;
                    if !settings.enabled.load(Ordering::Relaxed) {
                        continue;
                    }
                    crate::log::guarded("clipboard capture", || {
                        let mut src = foreground_exe();
                        if settings.is_ignored(&src) {
                            return;
                        }
                        if src == own {
                            src.clear(); // we put it there ourselves (paste from history): keep the original source
                        }
                        let Some(snap) = read_snapshot(
                            exclude_fmt,
                            png_fmt,
                            settings.images.load(Ordering::Relaxed),
                        ) else {
                            return;
                        };
                        let now = now_ms();
                        let stored = match snap {
                            Snapshot::Text(t) => store.add_text(&t, &src, now).ok().flatten(),
                            Snapshot::Png(p) => {
                                prepare_image(Some(&p), None).and_then(|(png, th, w, h)| {
                                    store.add_image(&png, &th, w, h, &src, now).ok().flatten()
                                })
                            }
                            Snapshot::Dib(d) => {
                                prepare_image(None, Some(&d)).and_then(|(png, th, w, h)| {
                                    store.add_image(&png, &th, w, h, &src, now).ok().flatten()
                                })
                            }
                        };
                        if stored.is_some() {
                            settings.prune(&store);
                            changed.store(true, Ordering::SeqCst);
                            wake();
                        }
                    });
                }
            })
            .expect("spawn clipboard watcher");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clipimg::{encode_png, png_dimensions, rgba_to_dib, Rgba};

    #[test]
    fn preview_and_ago() {
        assert_eq!(preview("\n  hello   world \nsecond", 80), "hello world");
        assert_eq!(preview(&"é".repeat(10), 4), "éééé…");
        assert_eq!(ago(100_000, 100_000), "刚刚");
        assert_eq!(ago(1_000_000, 100_000), "15 分钟前");
        assert_eq!(ago(100_000_000, 0), "1 天前");
    }

    #[test]
    fn ignore_list_matching() {
        let cfg = Config {
            clipboard_ignore_apps: vec!["KeePass.exe".into(), " 1password ".into(), "".into()],
            ..Config::default()
        };
        let s = ClipSettings::from_config(&cfg);
        assert!(s.is_ignored("keepass.exe"));
        assert!(s.is_ignored("1Password.EXE"));
        assert!(!s.is_ignored("chrome.exe"));
        assert!(!s.is_ignored(""));
    }

    #[test]
    fn image_preparation_from_dib_and_png() {
        let img = Rgba {
            w: 300,
            h: 150,
            px: [1u8, 2, 3, 255].repeat(300 * 150),
        };
        let (png, thumb, w, h) = prepare_image(None, Some(&rgba_to_dib(&img))).unwrap();
        assert_eq!((w, h), (300, 150));
        assert_eq!(png_dimensions(&thumb), Some((128, 64)));
        // A provided PNG is stored verbatim.
        let provided = encode_png(&img).unwrap();
        let (png2, _, w2, h2) = prepare_image(Some(&provided), None).unwrap();
        assert_eq!(png2, provided);
        assert_eq!((w2, h2), (300, 150));
        assert!(!png.is_empty());
        assert!(prepare_image(None, None).is_none());
        assert!(prepare_image(Some(b"junk"), None).is_none());
    }
}
