//! Browser bookmarks (Chrome, Edge, Brave, Vivaldi): read from each profile's `Bookmarks` JSON.
//! Firefox keeps bookmarks in SQLite and is not supported.

use serde_json::Value;
use std::path::PathBuf;

const MAX_BOOKMARKS: usize = 20_000;
const MAX_DEPTH: usize = 32;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bookmark {
    pub title: String,
    pub url: String,
    /// Folder path inside the browser, e.g. `书签栏 › 工作`.
    pub folder: String,
}

/// Host part of a URL without `www.`, e.g. `github.com`.
pub fn host(url: &str) -> &str {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let hostport = &rest[..end];
    let host = hostport.rsplit('@').next().unwrap_or(hostport);
    let host = host.split(':').next().unwrap_or(host);
    host.strip_prefix("www.").unwrap_or(host)
}

fn walk(node: &Value, path: &mut Vec<String>, out: &mut Vec<Bookmark>, depth: usize) {
    if depth > MAX_DEPTH || out.len() >= MAX_BOOKMARKS {
        return;
    }
    let name = node.get("name").and_then(Value::as_str).unwrap_or("");
    match node.get("type").and_then(Value::as_str) {
        Some("url") => {
            let url = node.get("url").and_then(Value::as_str).unwrap_or("");
            if url.starts_with("http://") || url.starts_with("https://") {
                let title = if name.trim().is_empty() { host(url) } else { name.trim() };
                out.push(Bookmark { title: title.to_string(), url: url.to_string(), folder: path.join(" › ") });
            }
        }
        _ => {
            if let Some(children) = node.get("children").and_then(Value::as_array) {
                let pushed = !name.is_empty();
                if pushed {
                    path.push(name.to_string());
                }
                for c in children {
                    walk(c, path, out, depth + 1);
                }
                if pushed {
                    path.pop();
                }
            }
        }
    }
}

/// Parse one `Bookmarks` file. Garbage in, empty list out.
pub fn parse(json: &[u8]) -> Vec<Bookmark> {
    let Ok(v) = serde_json::from_slice::<Value>(json) else { return Vec::new() };
    let mut out = Vec::new();
    if let Some(roots) = v.get("roots").and_then(Value::as_object) {
        for root in roots.values().filter(|r| r.is_object()) {
            walk(root, &mut Vec::new(), &mut out, 0);
        }
    }
    out
}

/// `Bookmarks` files of every installed Chromium-family browser profile.
pub fn profile_files() -> Vec<PathBuf> {
    let Some(local) = std::env::var_os("LOCALAPPDATA").map(PathBuf::from) else { return Vec::new() };
    let bases = [
        local.join(r"Google\Chrome\User Data"),
        local.join(r"Microsoft\Edge\User Data"),
        local.join(r"BraveSoftware\Brave-Browser\User Data"),
        local.join(r"Vivaldi\User Data"),
    ];
    let mut files = Vec::new();
    for base in bases {
        let Ok(rd) = std::fs::read_dir(&base) else { continue };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if name == "Default" || name.starts_with("Profile ") {
                let f = e.path().join("Bookmarks");
                if f.is_file() {
                    files.push(f);
                }
            }
        }
    }
    files
}

/// All bookmarks from all profiles, de-duplicated by URL, in browser order.
pub fn load_all() -> Vec<Bookmark> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for f in profile_files() {
        let Ok(bytes) = std::fs::read(&f) else { continue };
        for b in parse(&bytes) {
            if seen.insert(b.url.clone()) {
                out.push(b);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{
      "checksum": "x",
      "roots": {
        "bookmark_bar": { "name": "书签栏", "type": "folder", "children": [
          { "type": "url", "name": "GitHub", "url": "https://github.com/" },
          { "type": "folder", "name": "工作", "children": [
              { "type": "url", "name": "内部 Wiki", "url": "http://wiki.corp:8080/home" },
              { "type": "url", "name": "", "url": "https://www.example.com/a?b=1" } ] },
          { "type": "url", "name": "bad", "url": "javascript:alert(1)" },
          { "type": "url", "name": "internal", "url": "chrome://settings" }
        ] },
        "other": { "name": "其他书签", "type": "folder", "children": [] },
        "sync_transaction_version": "5"
      }
    }"#;

    #[test]
    fn parses_nested_folders_and_filters_schemes() {
        let b = parse(SAMPLE.as_bytes());
        let t: Vec<_> = b.iter().map(|x| (x.title.as_str(), x.folder.as_str())).collect();
        assert_eq!(t, [("GitHub", "书签栏"), ("内部 Wiki", "书签栏 › 工作"), ("example.com", "书签栏 › 工作")]);
        assert_eq!(b[1].url, "http://wiki.corp:8080/home");
    }

    #[test]
    fn garbage_and_empty_input_are_harmless() {
        assert!(parse(b"").is_empty());
        assert!(parse(b"not json").is_empty());
        assert!(parse(br#"{"roots": 5}"#).is_empty());
        assert!(parse(br#"{"roots": {"x": {"children": 3}}}"#).is_empty());
    }

    #[test]
    fn deep_nesting_is_cut_off() {
        let mut s = String::new();
        for _ in 0..200 {
            s.push_str(r#"{"name":"f","type":"folder","children":["#);
        }
        s.push_str(r#"{"type":"url","name":"deep","url":"https://deep.example/"}"#);
        for _ in 0..200 {
            s.push_str("]}");
        }
        let json = format!(r#"{{"roots":{{"r":{s}}}}}"#);
        assert!(parse(json.as_bytes()).is_empty()); // beyond MAX_DEPTH: ignored, no stack overflow
    }

    #[test]
    fn host_extraction() {
        assert_eq!(host("https://www.github.com/a/b"), "github.com");
        assert_eq!(host("http://wiki.corp:8080/x"), "wiki.corp");
        assert_eq!(host("https://user:pw@host.io/p"), "host.io");
        assert_eq!(host("example.org?q=1"), "example.org");
    }
}
