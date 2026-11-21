//! Real file and app icons from the Windows shell.
//!
//! Loading goes through shell extensions and the disk, so it happens on a background thread;
//! the UI asks for an icon, keeps drawing the coloured badge, and swaps in the picture when it
//! arrives. Pictures are cached by *key*: ordinary documents share one icon per extension
//! (`ext:.pdf`), folders share one, programs and shortcuts get their own.

use crate::types::{Action, Item, Kind};
use std::sync::mpsc::{channel, Receiver, Sender};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Spec {
    /// Cache key.
    pub key: String,
    /// What to ask the shell about (a real path, or a made-up name when `by_attrs`).
    pub path: String,
    /// Ask by file *attributes* only (no disk access): used for the generic per-extension and
    /// folder icons.
    pub by_attrs: bool,
    pub folder: bool,
}

pub struct IconImage {
    pub w: usize,
    pub h: usize,
    /// Straight (non-premultiplied) RGBA.
    pub rgba: Vec<u8>,
}

/// Extensions whose icon lives in the file itself (so each file needs its own lookup).
const PER_FILE: &[&str] = &["exe", "lnk", "ico", "msi", "scr", "url", "cpl", "appref-ms"];

/// Which icon to show for a result, if any. Store apps and settings keep the coloured badge.
pub fn spec_for(item: &Item) -> Option<Spec> {
    match item.kind {
        Kind::Folder => Some(Spec { key: "dir".into(), path: "folder".into(), by_attrs: true, folder: true }),
        Kind::File | Kind::App => {
            let path = item.path.clone().or_else(|| match &item.action {
                Action::Open { target, .. } => Some(target.clone()),
                _ => None,
            })?;
            if path.starts_with("shell:") || path.contains('!') || path.contains("://") {
                return None; // Store apps / URIs have no file to ask about
            }
            let ext = path.rsplit('.').next().filter(|e| !e.contains(['\\', '/'])).unwrap_or("").to_lowercase();
            if item.kind == Kind::App || PER_FILE.contains(&ext.as_str()) {
                Some(Spec { key: format!("p:{}", path.to_lowercase()), path, by_attrs: false, folder: false })
            } else {
                Some(Spec { key: format!("ext:.{ext}"), path: format!("file.{ext}"), by_attrs: true, folder: false })
            }
        }
        _ => None,
    }
}

/// Turn the shell's BGRA colour bitmap (+ optional AND-mask) into straight RGBA.
/// Icons without an alpha channel use the mask for transparency; with neither, fully opaque.
pub fn compose(w: usize, h: usize, mut bgra: Vec<u8>, mask: Option<&[u8]>) -> IconImage {
    let has_alpha = bgra.chunks_exact(4).any(|p| p[3] != 0);
    for (i, p) in bgra.chunks_exact_mut(4).enumerate() {
        p.swap(0, 2); // BGRA -> RGBA
        if !has_alpha {
            // Mask pixel: black = icon shows, white = transparent.
            let transparent = mask.and_then(|m| m.get(i * 4)).is_some_and(|&v| v != 0);
            p[3] = if transparent { 0 } else { 255 };
        }
    }
    IconImage { w, h, rgba: bgra }
}

pub struct IconLoader {
    tx: Sender<Spec>,
    pub rx: Receiver<(String, Option<IconImage>)>,
}

impl IconLoader {
    /// Start the worker. `provider` does the actual (slow) lookup; `wake` repaints the UI.
    pub fn spawn(provider: fn(&Spec) -> Option<IconImage>, init: fn(), wake: Box<dyn Fn() + Send>) -> Self {
        let (tx, req_rx) = channel::<Spec>();
        let (res_tx, rx) = channel();
        let _ = std::thread::Builder::new().name("icons".into()).spawn(move || {
            init();
            while let Ok(spec) = req_rx.recv() {
                let key = spec.key.clone();
                let img = std::panic::catch_unwind(|| provider(&spec)).unwrap_or(None);
                if res_tx.send((key, img)).is_err() {
                    break;
                }
                wake();
            }
        });
        Self { tx, rx }
    }

    pub fn request(&self, spec: Spec) {
        let _ = self.tx.send(spec);
    }

    /// The real shell-backed loader (on other platforms: always "no icon").
    pub fn spawn_system(wake: Box<dyn Fn() + Send>) -> Self {
        #[cfg(windows)]
        {
            Self::spawn(win::load, win::init_thread, wake)
        }
        #[cfg(not(windows))]
        {
            Self::spawn(|_| None, || {}, wake)
        }
    }
}

#[cfg(windows)]
mod win {
    use super::{compose, IconImage, Spec};
    use crate::winffi::*;
    use std::ffi::c_void;
    use std::mem::{size_of, zeroed};

    const SHGFI_ICON: u32 = 0x100;
    const SHGFI_LARGEICON: u32 = 0x0;
    const SHGFI_USEFILEATTRIBUTES: u32 = 0x10;
    const ATTR_DIRECTORY: u32 = 0x10;
    const ATTR_NORMAL: u32 = 0x80;

    pub fn init_thread() {
        // Shell icon handlers expect a COM apartment (COINIT_APARTMENTTHREADED = 2).
        unsafe {
            CoInitializeEx(std::ptr::null_mut(), 2);
        }
    }

    pub fn load(spec: &Spec) -> Option<IconImage> {
        unsafe {
            let mut info: SHFILEINFOW = zeroed();
            let mut flags = SHGFI_ICON | SHGFI_LARGEICON;
            let mut attrs = 0u32;
            if spec.by_attrs {
                flags |= SHGFI_USEFILEATTRIBUTES;
                attrs = if spec.folder { ATTR_DIRECTORY } else { ATTR_NORMAL };
            }
            let ok = SHGetFileInfoW(wide(&spec.path).as_ptr(), attrs, &mut info, size_of::<SHFILEINFOW>() as u32, flags);
            if ok == 0 || info.hIcon == 0 {
                return None;
            }
            let img = icon_to_image(info.hIcon);
            DestroyIcon(info.hIcon);
            img
        }
    }

    unsafe fn dib(hdc: isize, bitmap: isize, w: i32, h: i32) -> Option<Vec<u8>> {
        let mut bmi: BITMAPINFO = zeroed();
        bmi.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
        bmi.bmiHeader.biWidth = w;
        bmi.bmiHeader.biHeight = -h; // top-down
        bmi.bmiHeader.biPlanes = 1;
        bmi.bmiHeader.biBitCount = 32;
        let mut buf = vec![0u8; (w * h * 4) as usize];
        let lines = GetDIBits(hdc, bitmap, 0, h as u32, buf.as_mut_ptr() as *mut c_void, &mut bmi, 0);
        (lines != 0).then_some(buf)
    }

    unsafe fn icon_to_image(icon: isize) -> Option<IconImage> {
        let mut ii: ICONINFO = zeroed();
        if GetIconInfo(icon, &mut ii) == 0 {
            return None;
        }
        let result = (|| {
            if ii.hbmColor == 0 {
                return None;
            }
            let mut bm: BITMAP = zeroed();
            if GetObjectW(ii.hbmColor, size_of::<BITMAP>() as i32, &mut bm as *mut BITMAP as *mut c_void) == 0 {
                return None;
            }
            let (w, h) = (bm.bmWidth, bm.bmHeight);
            if w <= 0 || h <= 0 || w > 256 || h > 256 {
                return None;
            }
            let hdc = GetDC(0);
            if hdc == 0 {
                return None;
            }
            let color = dib(hdc, ii.hbmColor, w, h);
            let mask = if ii.hbmMask != 0 { dib(hdc, ii.hbmMask, w, h) } else { None };
            ReleaseDC(0, hdc);
            Some(compose(w as usize, h as usize, color?, mask.as_deref()))
        })();
        DeleteObject(ii.hbmColor);
        if ii.hbmMask != 0 {
            DeleteObject(ii.hbmMask);
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(kind: Kind, path: Option<&str>, target: &str) -> Item {
        Item {
            id: "x".into(),
            title: "t".into(),
            subtitle: String::new(),
            kind,
            action: Action::Open { target: target.into(), args: String::new(), admin: false, cwd: None },
            path: path.map(String::from),
            score: 0.0,
        }
    }

    #[test]
    fn documents_share_per_extension_icons_programs_get_their_own() {
        let a = spec_for(&item(Kind::File, Some(r"D:\a\Report.PDF"), "")).unwrap();
        let b = spec_for(&item(Kind::File, Some(r"E:\b\other.pdf"), "")).unwrap();
        assert_eq!(a.key, "ext:.pdf");
        assert_eq!(a.key, b.key);
        assert!(a.by_attrs && !a.folder);
        let exe = spec_for(&item(Kind::File, Some(r"C:\x\Tool.exe"), "")).unwrap();
        assert_eq!(exe.key, r"p:c:\x\tool.exe");
        assert!(!exe.by_attrs);
        let app = spec_for(&item(Kind::App, Some(r"C:\sm\Chrome.lnk"), "")).unwrap();
        assert!(!app.by_attrs);
        let folder = spec_for(&item(Kind::Folder, Some(r"C:\x"), "")).unwrap();
        assert_eq!((folder.key.as_str(), folder.folder), ("dir", true));
    }

    #[test]
    fn no_icon_for_store_apps_settings_and_extensionless_names_share_one() {
        assert!(spec_for(&item(Kind::App, None, r"shell:AppsFolder\Microsoft.Photos_8wekyb3d8bbwe!App")).is_none());
        assert!(spec_for(&item(Kind::Setting, None, "ms-settings:network")).is_none());
        assert!(spec_for(&item(Kind::Calc, None, "")).is_none());
        let n = spec_for(&item(Kind::File, Some(r"C:\a.b\README"), "")).unwrap();
        assert_eq!(n.key, "ext:."); // the dot in the folder name is not an extension
    }

    #[test]
    fn compose_converts_bgra_and_applies_mask_only_without_alpha() {
        // One pixel with alpha: B,G,R,A = 10,20,30,200 -> RGBA 30,20,10,200 (mask ignored).
        let img = compose(1, 1, vec![10, 20, 30, 200], Some(&[255, 255, 255, 0]));
        assert_eq!(img.rgba, [30, 20, 10, 200]);
        // No alpha anywhere: the mask decides (white = transparent, black = opaque).
        let img = compose(2, 1, vec![1, 2, 3, 0, 4, 5, 6, 0], Some(&[0, 0, 0, 0, 255, 255, 255, 0]));
        assert_eq!(img.rgba, [3, 2, 1, 255, 6, 5, 4, 0]);
        // No alpha and no mask: opaque.
        assert_eq!(compose(1, 1, vec![1, 2, 3, 0], None).rgba[3], 255);
    }

    #[test]
    fn loader_roundtrip_and_survives_a_panicking_provider() {
        fn provider(s: &Spec) -> Option<IconImage> {
            if s.key == "boom" {
                panic!("provider failure");
            }
            (s.key == "ok").then(|| IconImage { w: 1, h: 1, rgba: vec![1, 2, 3, 4] })
        }
        let _ = std::panic::take_hook(); // quiet test output
        let l = IconLoader::spawn(provider, || {}, Box::new(|| {}));
        let spec = |k: &str| Spec { key: k.into(), path: k.into(), by_attrs: false, folder: false };
        for k in ["boom", "ok", "none"] {
            l.request(spec(k));
        }
        let wait = || l.rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
        assert!(wait().1.is_none()); // boom -> None, worker still alive
        let (k, img) = wait();
        assert_eq!((k.as_str(), img.unwrap().rgba), ("ok", vec![1, 2, 3, 4]));
        assert!(wait().1.is_none());
    }
}
