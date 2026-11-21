//! Path browsing (like Alfred's file navigation): type `C:\Users\`, `~\Documents` or
//! `%appdata%` and the launcher lists that folder, filtered by what follows the last backslash.
//! Works without Everything, straight from the file system.
//!
//! Network paths (`\\server\share`) are deliberately not supported: listing an unreachable host
//! can block for a very long time.

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

/// Results from this module carry scores at or above this value (kept in the typed order, and
/// never fed to the learner).
pub const NAV_SCORE_BASE: f32 = 2000.0;
const MAX_ENTRIES: usize = 4000;
const MAX_SHOWN: usize = 60;
const LIST_TIMEOUT: Duration = Duration::from_millis(500);

#[derive(Debug, Clone, PartialEq)]
pub struct NavQuery {
    /// Directory to list, with the user's `~` / `%VAR%` already expanded.
    pub dir: PathBuf,
    /// Lowercased text after the last separator (empty = list everything).
    pub filter: String,
    /// True when the query ended with a separator, i.e. the folder itself was typed in full.
    pub trailing_sep: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NavEntry {
    pub name: String,
    pub is_dir: bool,
}

fn is_sep(c: char) -> bool {
    c == '\\' || c == '/'
}

/// Expand a leading `~` and any `%VAR%`. `None` when the text is not a path-like query.
fn expand(q: &str, env: &dyn Fn(&str) -> Option<String>) -> Option<String> {
    let mut s = q.to_string();
    if s == "~" || s.starts_with("~\\") || s.starts_with("~/") {
        let home = env("USERPROFILE").or_else(|| env("HOME"))?;
        s = format!("{}{}", home.trim_end_matches(is_sep), &s[1..]);
    }
    if s.contains('%') {
        let mut out = String::new();
        let mut rest = s.as_str();
        while let Some(i) = rest.find('%') {
            out.push_str(&rest[..i]);
            let after = &rest[i + 1..];
            let j = after.find('%')?; // unmatched %
            let name = &after[..j];
            if name.is_empty() {
                return None;
            }
            out.push_str(&env(name)?);
            rest = &after[j + 1..];
        }
        out.push_str(rest);
        s = out;
    }
    Some(s)
}

fn looks_like_path(q: &str) -> bool {
    let mut cs = q.chars();
    match (cs.next(), cs.next(), cs.next()) {
        (Some('~'), None, _) => true,
        (Some('~'), Some(c), _) if is_sep(c) => true,
        (Some('%'), _, _) => q[1..].find('%').is_some_and(|i| i > 0),
        (Some(d), Some(':'), Some(c)) => d.is_ascii_alphabetic() && is_sep(c),
        _ => false,
    }
}

/// Cheap check used to skip the Everything search for path-like input.
pub fn is_path_query(q: &str) -> bool {
    looks_like_path(unquote(q.trim()))
}

fn unquote(q: &str) -> &str {
    q.trim_matches('"')
}

pub fn parse(query: &str, env: &dyn Fn(&str) -> Option<String>) -> Option<NavQuery> {
    let q = unquote(query.trim());
    if !looks_like_path(q) || q.starts_with("\\\\") {
        return None;
    }
    let mut full = expand(q, env)?;
    // "~" or "%APPDATA%" alone mean "list that folder".
    let bare = q == "~" || (q.starts_with('%') && q.ends_with('%') && q[1..q.len() - 1].find('%').is_none());
    if bare {
        full.push('\\');
    }
    let idx = full.rfind(is_sep)?;
    let (dir, filter) = full.split_at(idx + 1);
    Some(NavQuery { dir: PathBuf::from(dir), filter: filter.to_lowercase(), trailing_sep: filter.is_empty() })
}

#[cfg(windows)]
fn is_hidden(md: &std::fs::Metadata, _name: &str) -> bool {
    use std::os::windows::fs::MetadataExt;
    md.file_attributes() & 0x6 != 0 // HIDDEN | SYSTEM
}

#[cfg(not(windows))]
fn is_hidden(_md: &std::fs::Metadata, name: &str) -> bool {
    name.starts_with('.')
}

/// Raw directory read (can be slow on dead network drives; see `list`).
fn read(dir: &Path) -> Vec<NavEntry> {
    let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut out = Vec::new();
    for e in rd.flatten().take(MAX_ENTRIES) {
        let name = e.file_name().to_string_lossy().into_owned();
        let Ok(md) = e.metadata() else { continue };
        out.push(NavEntry { is_dir: md.is_dir(), name: if is_hidden(&md, &name) { format!("\u{0}{name}") } else { name } });
    }
    out
}

/// Read `dir` on a helper thread so a dead drive cannot freeze the UI. `None` = timed out.
pub fn list(dir: &Path) -> Option<Vec<NavEntry>> {
    let (tx, rx) = mpsc::channel();
    let d = dir.to_path_buf();
    std::thread::spawn(move || {
        let _ = tx.send(read(&d));
    });
    rx.recv_timeout(LIST_TIMEOUT).ok()
}

/// Filter and order raw entries for display: folders first, name matches that start with the
/// filter before ones that merely contain it, then alphabetical. Hidden/system entries only show
/// when the filter starts with `.` or `$`.
pub fn select(entries: &[NavEntry], filter: &str) -> Vec<NavEntry> {
    let show_hidden = filter.starts_with('.') || filter.starts_with('$');
    let mut hits: Vec<(u8, bool, String, NavEntry)> = Vec::new();
    for e in entries {
        let (hidden, name) = match e.name.strip_prefix('\u{0}') {
            Some(n) => (true, n),
            None => (false, e.name.as_str()),
        };
        if hidden && !show_hidden {
            continue;
        }
        let lc = name.to_lowercase();
        let rank = if filter.is_empty() || lc == filter {
            0
        } else if lc.starts_with(filter) {
            1
        } else if lc.contains(filter) {
            2
        } else {
            continue;
        };
        hits.push((rank, !e.is_dir, lc, NavEntry { name: name.to_string(), is_dir: e.is_dir }));
    }
    hits.sort_by(|a, b| (a.0, a.1, &a.2).cmp(&(b.0, b.1, &b.2)));
    hits.truncate(MAX_SHOWN);
    hits.into_iter().map(|h| h.3).collect()
}

/// The new query text after accepting entry `name` (Tab / →): keeps the way the user wrote the
/// path (`~\`, `%APPDATA%\`) and adds a backslash after folders so you can keep going.
pub fn complete(typed: &str, name: &str, is_dir: bool) -> String {
    let t = unquote(typed.trim());
    let prefix = match t.rfind(is_sep) {
        Some(i) => &t[..=i],
        None => "", // "~" / "%VAR%" alone
    };
    let base = if prefix.is_empty() { format!("{t}\\") } else { prefix.to_string() };
    format!("{base}{name}{}", if is_dir { "\\" } else { "" })
}

/// One level up: `C:\a\b\c` and `C:\a\b\` both give `C:\a\b\`... then `C:\a\`.
pub fn parent_query(typed: &str) -> Option<String> {
    let t = unquote(typed.trim());
    let trimmed = t.trim_end_matches(is_sep);
    let i = trimmed.rfind(is_sep)?;
    Some(trimmed[..=i].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(n: &str) -> Option<String> {
        match n.to_ascii_uppercase().as_str() {
            "USERPROFILE" => Some(r"C:\Users\me".into()),
            "APPDATA" => Some(r"C:\Users\me\AppData\Roaming".into()),
            _ => None,
        }
    }

    #[test]
    fn detects_path_queries() {
        for q in [r"C:\", "c:/windows", r"d:\work\pro", "~", r"~\docs", "%appdata%", r"%appdata%\x", r#""C:\Program Files\""#] {
            assert!(is_path_query(q), "{q}");
        }
        for q in ["chrome", "c:", "c:x", r"\\server\share", "50%", "a~b", "", "~foo", "%%"] {
            assert!(!is_path_query(q), "{q}");
        }
    }

    #[test]
    fn parses_drive_home_and_env() {
        let p = parse(r"C:\Users\me\Doc", &env).unwrap();
        assert_eq!((p.dir, p.filter.as_str(), p.trailing_sep), (PathBuf::from(r"C:\Users\me\"), "doc", false));
        let p = parse(r"~\Documents\", &env).unwrap();
        assert_eq!((p.dir, p.filter.as_str(), p.trailing_sep), (PathBuf::from(r"C:\Users\me\Documents\"), "", true));
        let p = parse("~", &env).unwrap();
        assert_eq!((p.dir, p.trailing_sep), (PathBuf::from(r"C:\Users\me\"), true));
        let p = parse("%appdata%", &env).unwrap();
        assert_eq!(p.dir, PathBuf::from(r"C:\Users\me\AppData\Roaming\"));
        let p = parse(r#""C:\Program Files\App""#, &env).unwrap();
        assert_eq!(p.filter, "app");
        assert!(parse(r"%nosuchvar%\x", &env).is_none());
        assert!(parse("hello", &env).is_none());
    }

    fn e(name: &str, is_dir: bool) -> NavEntry {
        NavEntry { name: name.into(), is_dir }
    }

    #[test]
    fn select_orders_and_filters() {
        let all = vec![e("zeta.txt", false), e("Alpha", true), e("beta", true), e("my_alpha.txt", false), e("\u{0}.hidden", true), e("\u{0}$Recycle", true)];
        let names = |f: &str| select(&all, f).into_iter().map(|x| x.name).collect::<Vec<_>>();
        assert_eq!(names(""), ["Alpha", "beta", "my_alpha.txt", "zeta.txt"]); // folders first, hidden skipped
        assert_eq!(names("al"), ["Alpha", "my_alpha.txt"]); // prefix before contains
        assert_eq!(names(".")[0], ".hidden"); // prefix match first; then names merely containing "."
        assert_eq!(names("$"), ["$Recycle"]);
        assert!(names("nope").is_empty());
    }

    #[test]
    fn completion_keeps_the_typed_form() {
        assert_eq!(complete(r"C:\Users\me\Doc", "Documents", true), r"C:\Users\me\Documents\");
        assert_eq!(complete(r"~\Doc", "notes.txt", false), r"~\notes.txt");
        assert_eq!(complete("~", "Desktop", true), r"~\Desktop\");
        assert_eq!(complete("%appdata%", "Code", true), r"%appdata%\Code\");
        assert_eq!(complete(r#""C:\a\b""#, "bin", true), r"C:\a\bin\");
    }

    #[test]
    fn parent_navigation() {
        assert_eq!(parent_query(r"C:\a\b\c").as_deref(), Some(r"C:\a\b\"));
        assert_eq!(parent_query(r"C:\a\b\").as_deref(), Some(r"C:\a\"));
        assert_eq!(parent_query(r"C:\a\").as_deref(), Some(r"C:\"));
        assert_eq!(parent_query(r"C:\"), None);
    }

    #[test]
    fn lists_a_real_directory_and_survives_missing_ones() {
        let dir = std::env::temp_dir().join(format!("sf_nav_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("file.txt"), b"x").unwrap();
        #[cfg(not(windows))]
        std::fs::write(dir.join(".dot"), b"x").unwrap(); // dot-files count as hidden only off Windows
        let got = select(&list(&dir).unwrap(), "");
        let names: Vec<_> = got.iter().map(|x| (x.name.as_str(), x.is_dir)).collect();
        assert_eq!(names, [("sub", true), ("file.txt", false)]);
        assert!(list(&dir.join("missing")).unwrap().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
