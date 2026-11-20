//! Everything integration over its IPC (WM_COPYDATA, "QUERY2W").
//!
//! No DLL needed: we talk to the running Everything window directly. Works with Everything
//! 1.4 and 1.5 (both window classes are tried). The wire-format helpers are pure functions
//! with unit tests; only the window plumbing is Windows-specific.

use std::sync::mpsc::{Receiver, Sender};

pub const IPC_COPYDATA_QUERY2W: usize = 18;
pub const REQUEST_FULL_PATH_AND_NAME: u32 = 0x4;
pub const ITEM_FOLDER: u32 = 0x1;
pub const ITEM_DRIVE: u32 = 0x2;
pub const SORT_NAME_ASCENDING: u32 = 1;
/// dwData we ask Everything to use when replying; arbitrary but must match in our wndproc.
pub const REPLY_MESSAGE: u32 = 0x5346; // "SF"

#[derive(Clone, Debug, PartialEq)]
pub struct FileHit {
    pub path: String,
    pub is_folder: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum EvError {
    /// Everything.exe could not be located.
    NotInstalled,
    /// Installed but could not be started / did not respond.
    NotRunning,
    Other(String),
}

impl EvError {
    pub fn message(&self) -> String {
        match self {
            EvError::NotInstalled => {
                "未找到 Everything，请在设置中指定 Everything.exe 的路径".into()
            }
            EvError::NotRunning => "Everything 未运行或无响应".into(),
            EvError::Other(s) => s.clone(),
        }
    }
}

/// Build the QUERY2W request body (header + UTF-16 search text).
pub fn build_query2(reply_hwnd: u32, text: &str, offset: u32, max_results: u32) -> Vec<u8> {
    let mut b = Vec::with_capacity(28 + (text.len() + 1) * 2);
    for v in [
        reply_hwnd,
        REPLY_MESSAGE,
        0,
        offset,
        max_results,
        REQUEST_FULL_PATH_AND_NAME,
        SORT_NAME_ASCENDING,
    ] {
        b.extend_from_slice(&v.to_le_bytes());
    }
    for u in text.encode_utf16() {
        b.extend_from_slice(&u.to_le_bytes());
    }
    b.extend_from_slice(&[0, 0]);
    b
}

fn rd_u32(b: &[u8], off: usize) -> Option<u32> {
    b.get(off..off + 4)
        .map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

fn rd_wstr(b: &[u8], off: usize) -> Option<String> {
    let len = rd_u32(b, off)? as usize;
    let start = off + 4;
    let raw = b.get(start..start.checked_add(len.checked_mul(2)?)?)?;
    let units: Vec<u16> = raw
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    Some(String::from_utf16_lossy(&units))
}

/// Parse an EVERYTHING_IPC_LIST2 reply that was requested with FULL_PATH_AND_NAME only.
pub fn parse_list2(b: &[u8]) -> Result<Vec<FileHit>, String> {
    let num = rd_u32(b, 4).ok_or("short reply")? as usize;
    let req = rd_u32(b, 12).ok_or("short reply")?;
    if req & REQUEST_FULL_PATH_AND_NAME == 0 {
        return Err("unexpected request flags in reply".into());
    }
    let mut out = Vec::with_capacity(num);
    for i in 0..num {
        let base = 20 + i * 8;
        let (Some(flags), Some(data_off)) = (rd_u32(b, base), rd_u32(b, base + 4)) else {
            break;
        };
        let Some(path) = rd_wstr(b, data_off as usize) else {
            continue;
        };
        if path.is_empty() {
            continue;
        }
        out.push(FileHit {
            path,
            is_folder: flags & (ITEM_FOLDER | ITEM_DRIVE) != 0,
        });
    }
    Ok(out)
}

/// The text sent to Everything: the user's query plus negated excluded directories, so excluded
/// folders do not use up the limited result window. The query is wrapped in `<...>` so that an
/// `|` (OR) inside it cannot absorb the exclusions.
pub fn search_text(q: &str, excluded_dirs: &[String]) -> String {
    let excl: Vec<String> = excluded_dirs
        .iter()
        .map(|d| d.trim().trim_end_matches('\\'))
        .filter(|d| !d.is_empty() && !d.contains('"'))
        .take(30)
        .map(|d| format!("!\"{d}\\\""))
        .collect();
    if excl.is_empty() {
        q.to_string()
    } else {
        format!("<{q}> {}", excl.join(" "))
    }
}

pub struct SearchRequest {
    pub id: u64,
    pub text: String,
    pub max: u32,
}

pub struct SearchReply {
    pub id: u64,
    pub result: Result<Vec<FileHit>, EvError>,
}

/// Candidate install locations, checked in order after the user-configured path.
pub fn candidate_paths() -> Vec<std::path::PathBuf> {
    use std::path::PathBuf;
    let mut v = Vec::new();
    for var in ["ProgramFiles", "ProgramFiles(x86)", "ProgramW6432"] {
        if let Some(p) = std::env::var_os(var) {
            for dir in ["Everything", "Everything 1.5a", "Everything 1.5"] {
                v.push(PathBuf::from(&p).join(dir).join("Everything.exe"));
                v.push(PathBuf::from(&p).join(dir).join("Everything64.exe"));
            }
        }
    }
    if let Some(p) = std::env::var_os("LOCALAPPDATA") {
        v.push(PathBuf::from(&p).join(r"Programs\Everything\Everything.exe"));
        v.push(PathBuf::from(&p).join(r"Programs\Everything 1.5a\Everything.exe"));
    }
    if let Some(path) = std::env::var_os("PATH") {
        for d in std::env::split_paths(&path) {
            v.push(d.join("Everything.exe"));
        }
    }
    v
}

pub fn find_exe(configured: &str) -> Option<std::path::PathBuf> {
    let c = configured.trim().trim_matches('"');
    if !c.is_empty() {
        let p = std::path::PathBuf::from(c);
        if p.is_file() {
            return Some(p);
        }
        // Allow pointing at the install folder instead of the exe.
        for n in ["Everything.exe", "Everything64.exe"] {
            let q = p.join(n);
            if q.is_file() {
                return Some(q);
            }
        }
    }
    candidate_paths().into_iter().find(|p| p.is_file())
}

#[cfg(windows)]
mod imp {
    use super::*;
    use crate::winffi::*;
    use std::cell::RefCell;
    use std::time::{Duration, Instant};

    thread_local! {
        static REPLY: RefCell<Option<Vec<u8>>> = const { RefCell::new(None) };
    }

    unsafe extern "system" fn wndproc(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
        if m == WM_COPYDATA && l != 0 {
            let cds = &*(l as *const COPYDATASTRUCT);
            if cds.dwData == REPLY_MESSAGE as usize && !cds.lpData.is_null() {
                let bytes =
                    std::slice::from_raw_parts(cds.lpData as *const u8, cds.cbData as usize)
                        .to_vec();
                REPLY.with(|r| *r.borrow_mut() = Some(bytes));
                return 1;
            }
        }
        DefWindowProcW(h, m, w, l)
    }

    pub struct Client {
        hwnd: HWND,
    }

    impl Client {
        pub fn new() -> Result<Self, EvError> {
            unsafe {
                let hinst = GetModuleHandleW(std::ptr::null());
                let class = wide("SwiftFindEverythingIpc");
                let wc = WNDCLASSW {
                    style: 0,
                    lpfnWndProc: Some(wndproc),
                    cbClsExtra: 0,
                    cbWndExtra: 0,
                    hInstance: hinst,
                    hIcon: 0,
                    hCursor: 0,
                    hbrBackground: 0,
                    lpszMenuName: std::ptr::null(),
                    lpszClassName: class.as_ptr(),
                };
                RegisterClassW(&wc); // fails harmlessly if already registered
                let title = wide("");
                let hwnd = CreateWindowExW(
                    0,
                    class.as_ptr(),
                    title.as_ptr(),
                    0,
                    0,
                    0,
                    0,
                    0,
                    0,
                    0,
                    hinst,
                    std::ptr::null_mut(),
                );
                if hwnd == 0 {
                    return Err(EvError::Other("failed to create IPC window".into()));
                }
                Ok(Self { hwnd })
            }
        }

        fn find_target() -> HWND {
            unsafe {
                for class in [
                    "EVERYTHING_TASKBAR_NOTIFICATION",
                    "EVERYTHING_TASKBAR_NOTIFICATION_(1.5a)",
                ] {
                    let c = wide(class);
                    let h = FindWindowW(c.as_ptr(), std::ptr::null());
                    if h != 0 {
                        return h;
                    }
                }
                0
            }
        }

        /// Make sure Everything is running; start it hidden (tray) if it is not.
        pub fn ensure_running(&self, configured_path: &str) -> Result<HWND, EvError> {
            let h = Self::find_target();
            if h != 0 {
                return Ok(h);
            }
            let exe = find_exe(configured_path).ok_or(EvError::NotInstalled)?;
            unsafe {
                let file = wide(&exe.to_string_lossy());
                let verb = wide("open");
                let args = wide("-startup");
                ShellExecuteW(
                    0,
                    verb.as_ptr(),
                    file.as_ptr(),
                    args.as_ptr(),
                    std::ptr::null(),
                    SW_HIDE,
                );
            }
            let deadline = Instant::now() + Duration::from_secs(6);
            while Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(100));
                let h = Self::find_target();
                if h != 0 {
                    return Ok(h);
                }
            }
            Err(EvError::NotRunning)
        }

        fn pump(&self) {
            unsafe {
                let mut msg = MSG::default();
                while PeekMessageW(&mut msg, 0, 0, 0, PM_REMOVE) != 0 {
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
        }

        pub fn query(&self, target: HWND, text: &str, max: u32) -> Result<Vec<FileHit>, EvError> {
            REPLY.with(|r| *r.borrow_mut() = None);
            let buf = build_query2(self.hwnd as u32, text, 0, max);
            let cds = COPYDATASTRUCT {
                dwData: IPC_COPYDATA_QUERY2W,
                cbData: buf.len() as u32,
                lpData: buf.as_ptr() as *mut _,
            };
            let mut res = 0usize;
            let ok = unsafe {
                SendMessageTimeoutW(
                    target,
                    WM_COPYDATA,
                    self.hwnd as WPARAM,
                    &cds as *const _ as LPARAM,
                    SMTO_ABORTIFHUNG,
                    3000,
                    &mut res,
                )
            };
            if ok == 0 {
                return Err(EvError::NotRunning);
            }
            let deadline = Instant::now() + Duration::from_millis(2500);
            loop {
                if let Some(bytes) = REPLY.with(|r| r.borrow_mut().take()) {
                    return parse_list2(&bytes).map_err(EvError::Other);
                }
                self.pump();
                if Instant::now() > deadline {
                    return Err(EvError::Other("Everything 响应超时".into()));
                }
                std::thread::sleep(Duration::from_millis(1));
            }
        }
    }

    impl Drop for Client {
        fn drop(&mut self) {
            unsafe {
                DestroyWindow(self.hwnd);
            }
        }
    }
}

/// Background worker: receives search requests, keeps only the newest, answers over `tx`.
/// `wake` is called after every reply so the UI can repaint.
pub fn spawn_worker(
    path: std::sync::Arc<std::sync::Mutex<String>>,
    rx: Receiver<SearchRequest>,
    tx: Sender<SearchReply>,
    wake: Box<dyn Fn() + Send>,
) {
    std::thread::Builder::new()
        .name("everything-ipc".into())
        .spawn(move || worker_main(path, rx, tx, wake))
        .expect("spawn everything worker");
}

#[cfg(windows)]
fn worker_main(
    path: std::sync::Arc<std::sync::Mutex<String>>,
    rx: Receiver<SearchRequest>,
    tx: Sender<SearchReply>,
    wake: Box<dyn Fn() + Send>,
) {
    let client = match imp::Client::new() {
        Ok(c) => c,
        Err(e) => {
            while let Ok(req) = rx.recv() {
                let _ = tx.send(SearchReply {
                    id: req.id,
                    result: Err(e.clone()),
                });
                wake();
            }
            return;
        }
    };
    let mut cached_target: crate::winffi::HWND = 0;
    while let Ok(mut req) = rx.recv() {
        // Latest-wins: skip stale requests that piled up while we were busy.
        while let Ok(newer) = rx.try_recv() {
            req = newer;
        }
        let configured = path.lock().map(|p| p.clone()).unwrap_or_default();
        let attempt = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            (|| -> Result<Vec<FileHit>, EvError> {
                if cached_target == 0 {
                    cached_target = client.ensure_running(&configured)?;
                }
                match client.query(cached_target, &req.text, req.max) {
                    Err(EvError::NotRunning) => {
                        // Everything was restarted; re-resolve its window and retry once.
                        cached_target = client.ensure_running(&configured)?;
                        client.query(cached_target, &req.text, req.max)
                    }
                    other => other,
                }
            })()
        }));
        let result = attempt.unwrap_or_else(|_| {
            crate::log::info("recovered from panic in Everything query");
            cached_target = 0;
            Err(EvError::Other("Everything 查询出错(已记录到日志)".into()))
        });
        let _ = tx.send(SearchReply { id: req.id, result });
        wake();
    }
}

#[cfg(not(windows))]
fn worker_main(
    _path: std::sync::Arc<std::sync::Mutex<String>>,
    rx: Receiver<SearchRequest>,
    tx: Sender<SearchReply>,
    wake: Box<dyn Fn() + Send>,
) {
    while let Ok(req) = rx.recv() {
        let _ = tx.send(SearchReply {
            id: req.id,
            result: Err(EvError::Other("Everything 仅支持 Windows".into())),
        });
        wake();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn push_str(buf: &mut Vec<u8>, s: &str) -> u32 {
        let off = buf.len() as u32;
        let units: Vec<u16> = s.encode_utf16().collect();
        buf.extend_from_slice(&(units.len() as u32).to_le_bytes());
        for u in &units {
            buf.extend_from_slice(&u.to_le_bytes());
        }
        buf.extend_from_slice(&[0, 0]);
        off
    }

    #[test]
    fn exclusions_are_appended_as_negated_paths() {
        assert_eq!(search_text("foo", &[]), "foo");
        let ex = vec![
            r"D:\Games\".to_string(),
            r"C:\$Recycle.Bin".to_string(),
            String::new(),
        ];
        assert_eq!(
            search_text("a | b", &ex),
            r#"<a | b> !"D:\Games\" !"C:\$Recycle.Bin\""#
        );
    }

    #[test]
    fn query_layout() {
        let q = build_query2(0x1234, "ab", 0, 50);
        assert_eq!(q.len(), 28 + 6);
        assert_eq!(&q[0..4], &0x1234u32.to_le_bytes());
        assert_eq!(&q[16..20], &50u32.to_le_bytes());
        assert_eq!(&q[28..34], &[b'a', 0, b'b', 0, 0, 0]);
    }

    #[test]
    fn parse_reply_roundtrip() {
        // Header (20 bytes) + 2 items (16 bytes) + strings.
        let mut b = Vec::new();
        for v in [2u32, 2, 0, REQUEST_FULL_PATH_AND_NAME, 1] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        let items_at = b.len();
        b.extend_from_slice(&[0u8; 16]);
        let o1 = push_str(&mut b, r"C:\Users\me\报告.docx");
        let o2 = push_str(&mut b, r"C:\Temp");
        b[items_at..items_at + 4].copy_from_slice(&0u32.to_le_bytes());
        b[items_at + 4..items_at + 8].copy_from_slice(&o1.to_le_bytes());
        b[items_at + 8..items_at + 12].copy_from_slice(&ITEM_FOLDER.to_le_bytes());
        b[items_at + 12..items_at + 16].copy_from_slice(&o2.to_le_bytes());
        let hits = parse_list2(&b).unwrap();
        assert_eq!(hits.len(), 2);
        assert_eq!(
            hits[0],
            FileHit {
                path: r"C:\Users\me\报告.docx".into(),
                is_folder: false
            }
        );
        assert!(hits[1].is_folder);
    }

    #[test]
    fn truncated_reply_does_not_panic() {
        assert!(parse_list2(&[1, 2, 3]).is_err());
        let mut b = Vec::new();
        for v in [1u32, 1, 0, REQUEST_FULL_PATH_AND_NAME, 1, 0, 9999] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        assert_eq!(parse_list2(&b).unwrap().len(), 0);
    }
}
