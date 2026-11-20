//! Installed application index: Start Menu shortcuts + UWP/Store apps.
//!
//! Start Menu shortcuts are found by walking the shortcut folders (fast, no subprocess).
//! Store apps come from `Get-StartApps`, run once in a background thread and cached on disk
//! so the next launch has them instantly.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct AppEntry {
    pub name: String,
    /// `.lnk` path, or `shell:AppsFolder\<AUMID>` for Store apps.
    pub target: String,
    pub uwp: bool,
}

const SKIP_WORDS: &[&str] = &[
    "uninstall",
    "卸载",
    "readme",
    "read me",
    "license",
    "release notes",
    "documentation",
    "帮助",
    "help",
    "website",
    "官网",
    "更新日志",
];

fn skip_name(name: &str) -> bool {
    let n = name.to_lowercase();
    SKIP_WORDS.iter().any(|w| n.contains(w))
}

fn walk(dir: &Path, out: &mut Vec<AppEntry>, depth: u8) {
    if depth > 5 {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        let Ok(ft) = e.file_type() else { continue };
        if ft.is_dir() {
            walk(&p, out, depth + 1);
        } else if p
            .extension()
            .and_then(|x| x.to_str())
            .map(|x| x.eq_ignore_ascii_case("lnk"))
            .unwrap_or(false)
        {
            if let Some(stem) = p.file_stem().and_then(|s| s.to_str()) {
                if !skip_name(stem) {
                    out.push(AppEntry {
                        name: stem.to_string(),
                        target: p.to_string_lossy().into_owned(),
                        uwp: false,
                    });
                }
            }
        }
    }
}

pub fn scan_shortcuts() -> Vec<AppEntry> {
    let mut roots: Vec<PathBuf> = Vec::new();
    if let Some(p) = std::env::var_os("ProgramData") {
        roots.push(PathBuf::from(p).join(r"Microsoft\Windows\Start Menu\Programs"));
    }
    if let Some(p) = std::env::var_os("APPDATA") {
        roots.push(PathBuf::from(p).join(r"Microsoft\Windows\Start Menu\Programs"));
    }
    if let Some(p) = std::env::var_os("USERPROFILE") {
        roots.push(PathBuf::from(p).join("Desktop"));
    }
    if let Some(p) = std::env::var_os("PUBLIC") {
        roots.push(PathBuf::from(p).join("Desktop"));
    }
    let mut out = Vec::new();
    for r in roots {
        walk(&r, &mut out, 0);
    }
    out
}

/// Parse the CSV produced by `Get-StartApps | ConvertTo-Csv -NoTypeInformation`.
pub fn parse_start_apps_csv(text: &str) -> Vec<AppEntry> {
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let line = line.trim_start_matches('\u{feff}').trim();
        if line.is_empty() || i == 0 && line.starts_with("\"Name\"") {
            continue;
        }
        let fields = parse_csv_line(line);
        if fields.len() < 2 {
            continue;
        }
        let (name, id) = (fields[0].trim(), fields[1].trim());
        if name.is_empty() || id.is_empty() || skip_name(name) {
            continue;
        }
        // Classic desktop apps appear here too (AppID is a path) – the .lnk scan already has them.
        let is_aumid = id.contains('!') && !id.contains('\\');
        if !is_aumid {
            continue;
        }
        out.push(AppEntry {
            name: name.to_string(),
            target: format!(r"shell:AppsFolder\{id}"),
            uwp: true,
        });
    }
    out
}

fn parse_csv_line(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut cur = String::new();
    let mut in_q = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' if in_q && chars.peek() == Some(&'"') => {
                cur.push('"');
                chars.next();
            }
            '"' => in_q = !in_q,
            ',' if !in_q => fields.push(std::mem::take(&mut cur)),
            _ => cur.push(c),
        }
    }
    fields.push(cur);
    fields
}

#[cfg(windows)]
pub fn scan_store_apps() -> Vec<AppEntry> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let out = std::process::Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "[Console]::OutputEncoding=[Text.Encoding]::UTF8; Get-StartApps | ConvertTo-Csv -NoTypeInformation",
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output();
    match out {
        Ok(o) if o.status.success() => parse_start_apps_csv(&String::from_utf8_lossy(&o.stdout)),
        _ => Vec::new(),
    }
}

#[cfg(not(windows))]
pub fn scan_store_apps() -> Vec<AppEntry> {
    Vec::new()
}

/// Merge shortcut apps and Store apps; shortcut wins on duplicate names.
pub fn merge(shortcuts: Vec<AppEntry>, store: Vec<AppEntry>) -> Vec<AppEntry> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::with_capacity(shortcuts.len() + store.len());
    for a in shortcuts.into_iter().chain(store) {
        if seen.insert(a.name.to_lowercase()) {
            out.push(a);
        }
    }
    out.sort_by_cached_key(|a| a.name.to_lowercase());
    out
}

fn cache_path() -> PathBuf {
    crate::config::data_dir().join("apps_cache.json")
}

pub fn load_cache() -> Vec<AppEntry> {
    std::fs::read(cache_path())
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

pub fn save_cache(apps: &[AppEntry]) {
    let _ = std::fs::create_dir_all(crate::config::data_dir());
    if let Ok(b) = serde_json::to_vec(apps) {
        let _ = crate::config::write_atomic(&cache_path(), &b);
    }
}

/// Full rescan; intended to run on a background thread.
pub fn rescan() -> Vec<AppEntry> {
    let apps = merge(scan_shortcuts(), scan_store_apps());
    save_cache(&apps);
    apps
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_parsing_filters_and_formats() {
        let csv = "\"Name\",\"AppID\"\n\
                   \"计算器\",\"Microsoft.WindowsCalculator_8wekyb3d8bbwe!App\"\n\
                   \"Notepad\",\"C:\\Windows\\notepad.exe\"\n\
                   \"Say \"\"hi\"\"\",\"Pkg_1!App\"\n\
                   \"Uninstall Thing\",\"Pkg_2!App\"\n";
        let apps = parse_start_apps_csv(csv);
        assert_eq!(apps.len(), 2);
        assert_eq!(apps[0].name, "计算器");
        assert!(apps[0].target.starts_with("shell:AppsFolder\\"));
        assert_eq!(apps[1].name, "Say \"hi\"");
    }

    #[test]
    fn merge_prefers_shortcuts() {
        let a = AppEntry {
            name: "Foo".into(),
            target: "a.lnk".into(),
            uwp: false,
        };
        let b = AppEntry {
            name: "foo".into(),
            target: "shell:AppsFolder\\x".into(),
            uwp: true,
        };
        let m = merge(vec![a], vec![b]);
        assert_eq!(m.len(), 1);
        assert!(!m[0].uwp);
    }
}
