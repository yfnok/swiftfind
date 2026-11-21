//! "Recent files": what Windows itself remembers you opened (the `Recent` folder of `.lnk`
//! shortcuts), newest first. Type `recent` or `最近`, optionally followed by words to filter.
//!
//! A shortcut is a binary "Shell Link". We read just enough of it (the LinkInfo block) to get the
//! target path, without COM. Targets on network shares are skipped: probing them can hang.

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, SystemTime};

const MAX_LINKS: usize = 150;
const TIMEOUT: Duration = Duration::from_millis(900);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recent {
    pub path: String,
    pub is_dir: bool,
}

pub fn recent_dir() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join(r"Microsoft\Windows\Recent"))
}

fn u32_at(b: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(o..o + 4)?.try_into().ok()?))
}

fn utf16_at(b: &[u8], o: usize) -> Option<String> {
    let rest = b.get(o..)?;
    let units: Vec<u16> = rest.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).take_while(|u| *u != 0).collect();
    Some(String::from_utf16_lossy(&units))
}

/// ANSI strings are only trusted when they are plain ASCII (the code page is unknown).
fn ascii_at(b: &[u8], o: usize) -> Option<String> {
    let rest = b.get(o..)?;
    let end = rest.iter().position(|c| *c == 0)?;
    let s = &rest[..end];
    s.is_ascii().then(|| String::from_utf8_lossy(s).into_owned())
}

/// Target path stored in a `.lnk` file, if it points at a local file or folder.
pub fn lnk_target(b: &[u8]) -> Option<String> {
    if u32_at(b, 0)? != 0x4C {
        return None;
    }
    let flags = u32_at(b, 0x14)?;
    let mut off = 0x4C;
    if flags & 0x1 != 0 {
        // HasLinkTargetIDList: u16 size + that many bytes.
        let n = u16::from_le_bytes(b.get(off..off + 2)?.try_into().ok()?) as usize;
        off += 2 + n;
    }
    if flags & 0x2 == 0 {
        return None; // no LinkInfo
    }
    let info = b.get(off..)?;
    let header_size = u32_at(info, 4)? as usize;
    let info_flags = u32_at(info, 8)?;
    if info_flags & 0x1 == 0 {
        return None; // not a local path (network share)
    }
    let base_off = u32_at(info, 16)? as usize;
    let suffix_off = u32_at(info, 24)? as usize;
    let (base, suffix) = if header_size >= 0x24 {
        let ub = u32_at(info, 28)? as usize;
        let us = u32_at(info, 32)? as usize;
        (utf16_at(info, ub), utf16_at(info, us))
    } else {
        (ascii_at(info, base_off), ascii_at(info, suffix_off))
    };
    // Both parts must be readable; a dropped suffix would silently turn a file into its folder.
    let (base, suffix) = (base?, suffix?);
    let full = format!("{base}{suffix}");
    (full.len() > 2 && !full.starts_with("\\\\")).then_some(full)
}

/// Newest-first recent items from `dir`. Runs the file-system probing on a helper thread with a
/// time limit; `None` on timeout.
pub fn list(dir: &Path) -> Option<Vec<Recent>> {
    let (tx, rx) = mpsc::channel();
    let d = dir.to_path_buf();
    std::thread::spawn(move || {
        let _ = tx.send(read(&d));
    });
    rx.recv_timeout(TIMEOUT).ok()
}

fn read(dir: &Path) -> Vec<Recent> {
    let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut links: Vec<(SystemTime, PathBuf)> = rd
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x.eq_ignore_ascii_case("lnk")))
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .collect();
    links.sort_by_key(|l| std::cmp::Reverse(l.0));
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for (_, p) in links.into_iter().take(MAX_LINKS) {
        let Ok(bytes) = std::fs::read(&p) else { continue };
        let Some(target) = lnk_target(&bytes) else { continue };
        let tp = Path::new(&target);
        let Ok(md) = std::fs::metadata(tp) else { continue }; // deleted or unreachable
        if seen.insert(target.to_lowercase()) {
            out.push(Recent { path: target, is_dir: md.is_dir() });
        }
    }
    out
}

/// `Some(filter)` when the query asks for recent files: `recent`, `最近`, `recent report`.
pub fn trigger(q: &str) -> Option<String> {
    let mut it = q.splitn(2, char::is_whitespace);
    let head = it.next()?.to_lowercase();
    (head == "recent" || head == "最近").then(|| it.next().unwrap_or("").trim().to_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal Shell Link with a LinkInfo block (optionally with Unicode path fields).
    fn make_lnk(base: &str, suffix: &str, unicode: bool, local: bool) -> Vec<u8> {
        let mut b = vec![0u8; 0x4C];
        b[0..4].copy_from_slice(&0x4Cu32.to_le_bytes());
        b[0x14..0x18].copy_from_slice(&2u32.to_le_bytes()); // HasLinkInfo
        let header = if unicode { 0x24u32 } else { 0x1C };
        let mut tail: Vec<u8> = Vec::new(); // strings, offsets relative to LinkInfo start
        let base_ansi = header as usize;
        tail.extend_from_slice(base.as_bytes());
        tail.push(0);
        let suffix_ansi = base_ansi + tail.len();
        tail.extend_from_slice(suffix.as_bytes());
        tail.push(0);
        let base_uni = base_ansi + tail.len();
        for u in base.encode_utf16() {
            tail.extend_from_slice(&u.to_le_bytes());
        }
        tail.extend_from_slice(&[0, 0]);
        let suffix_uni = base_ansi + tail.len();
        for u in suffix.encode_utf16() {
            tail.extend_from_slice(&u.to_le_bytes());
        }
        tail.extend_from_slice(&[0, 0]);
        let mut info = Vec::new();
        info.extend_from_slice(&((header as usize + tail.len()) as u32).to_le_bytes());
        info.extend_from_slice(&header.to_le_bytes());
        info.extend_from_slice(&(if local { 1u32 } else { 2u32 }).to_le_bytes());
        info.extend_from_slice(&0u32.to_le_bytes()); // VolumeIDOffset
        info.extend_from_slice(&(base_ansi as u32).to_le_bytes());
        info.extend_from_slice(&0u32.to_le_bytes()); // CommonNetworkRelativeLinkOffset
        info.extend_from_slice(&(suffix_ansi as u32).to_le_bytes());
        if unicode {
            info.extend_from_slice(&(base_uni as u32).to_le_bytes());
            info.extend_from_slice(&(suffix_uni as u32).to_le_bytes());
        }
        info.extend_from_slice(&tail);
        b.extend_from_slice(&info);
        b
    }

    #[test]
    fn reads_unicode_target_including_chinese() {
        let l = make_lnk(r"D:\工作\", "会议纪要.docx", true, true);
        assert_eq!(lnk_target(&l).as_deref(), Some(r"D:\工作\会议纪要.docx"));
    }

    #[test]
    fn ansi_only_works_for_ascii_and_refuses_guessing() {
        let l = make_lnk(r"C:\Docs\", "a.txt", false, true);
        assert_eq!(lnk_target(&l).as_deref(), Some(r"C:\Docs\a.txt"));
        // ANSI with non-ASCII bytes: code page unknown, so no result rather than mojibake.
        let mut l = make_lnk(r"C:\Docs\", "a.txt", false, true);
        let pos = l.windows(5).position(|w| w == b"a.txt").unwrap();
        l[pos] = 0xC4;
        assert_eq!(lnk_target(&l), None);
    }

    #[test]
    fn network_and_garbage_are_ignored() {
        assert_eq!(lnk_target(&make_lnk(r"\\srv\share\", "x.txt", true, false)), None);
        assert_eq!(lnk_target(&make_lnk(r"\\srv\", "x", true, true)), None);
        assert_eq!(lnk_target(&[1, 2, 3]), None);
        assert_eq!(lnk_target(&[0u8; 200]), None);
        let mut cut = make_lnk(r"C:\a\", "b.txt", true, true);
        cut.truncate(0x4C + 10);
        assert_eq!(lnk_target(&cut), None);
    }

    #[test]
    fn trigger_words() {
        assert_eq!(trigger("recent").as_deref(), Some(""));
        assert_eq!(trigger("最近 报告").as_deref(), Some("报告"));
        assert_eq!(trigger("Recent  Foo").as_deref(), Some("foo"));
        assert_eq!(trigger("recently"), None);
        assert_eq!(trigger("chrome"), None);
    }

    #[test]
    fn reads_a_folder_newest_first_and_skips_missing_targets() {
        let dir = std::env::temp_dir().join(format!("sf_recent_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let real_a = dir.join("a.txt");
        let real_b = dir.join("b.txt");
        std::fs::write(&real_a, b"x").unwrap();
        std::fs::write(&real_b, b"x").unwrap();
        let mk = |name: &str, target: &Path| {
            let s = target.to_string_lossy();
            let (base, suffix) = s.rsplit_once('/').unwrap();
            std::fs::write(dir.join(name), make_lnk(&format!("{base}/"), suffix, true, true)).unwrap();
        };
        mk("a.lnk", &real_a);
        std::thread::sleep(Duration::from_millis(30));
        mk("gone.lnk", &dir.join("deleted.txt"));
        std::thread::sleep(Duration::from_millis(30));
        mk("b.lnk", &real_b);
        let got = list(&dir).unwrap();
        let names: Vec<_> = got.iter().map(|r| r.path.rsplit('/').next().unwrap().to_string()).collect();
        assert_eq!(names, ["b.txt", "a.txt"]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
