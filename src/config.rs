//! User configuration, stored as pretty JSON in `%APPDATA%\SwiftFind\config.json`.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub struct Snippet {
    pub name: String,
    pub text: String,
    /// Extra words that also find this snippet.
    pub keywords: Vec<String>,
}

fn snippet(name: &str, text: &str, kw: &[&str]) -> Snippet {
    Snippet { name: name.into(), text: text.into(), keywords: kw.iter().map(|s| s.to_string()).collect() }
}

/// Built-in examples; they also show how the feature works.
pub fn default_snippets() -> Vec<Snippet> {
    vec![
        snippet("日期", "{date}", &["date", "today", "rq", "今天"]),
        snippet("时间", "{datetime}", &["time", "now", "sj", "现在"]),
    ]
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CustomCommand {
    /// Trigger word typed in the search box (e.g. `cmd`).
    pub name: String,
    /// Title shown in the result list.
    pub title: String,
    pub program: String,
    /// Arguments; `{q}` is replaced by whatever was typed after the trigger word.
    #[serde(default)]
    pub args: String,
    /// Launch elevated (UAC prompt).
    #[serde(default)]
    pub admin: bool,
    #[serde(default)]
    pub cwd: Option<String>,
    /// Ask for a second Enter before running (shutdown, kill …).
    #[serde(default)]
    pub confirm: bool,
    /// Extra words that also find this command.
    #[serde(default)]
    pub keywords: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct WebSearch {
    pub key: String,
    pub name: String,
    /// URL template; `{q}` is replaced by the URL-encoded query.
    pub url: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Config {
    /// Full path to Everything.exe. Empty = auto-detect.
    pub everything_path: String,
    /// Global hotkey, e.g. `Alt+Space`, `Ctrl+Alt+F`, `Win+Space`.
    pub hotkey: String,
    /// Global hotkey that opens the launcher directly on the clipboard history (empty = off).
    pub clipboard_hotkey: String,
    /// Record clipboard history (stored in a local database).
    pub clipboard_enabled: bool,
    /// Also record copied images (kept as PNG with a thumbnail).
    pub clipboard_images: bool,
    /// Keep at most this many unpinned items.
    pub clipboard_max: usize,
    /// Delete unpinned items older than this many days (0 = keep forever).
    pub clipboard_days: u32,
    /// Total size limit for stored images in MB (0 = unlimited). Oldest unpinned images go first.
    pub clipboard_image_mb: u32,
    /// Do not record anything copied from these programs (exe names, e.g. `keepass.exe`).
    pub clipboard_ignore_apps: Vec<String>,
    /// Rows visible at once.
    pub visible_rows: usize,
    /// Max results requested from Everything.
    pub max_file_results: u32,
    /// Hide the window when it loses focus.
    pub hide_on_blur: bool,
    /// Show your most-used items when the search box is empty (off = just the clean search bar).
    pub show_recent_on_empty: bool,
    /// Where the user dragged the window to (top-left, screen pixels). `None` = centered.
    pub window_pos: Option<(i32, i32)>,
    /// Which set of built-in commands this config has already received (see `migrate`).
    #[serde(default = "zero")] // a file written before this field existed is version 0
    pub defaults_version: u32,
    /// Path prefixes that never appear in results.
    pub excluded_dirs: Vec<String>,
    pub show_settings: bool,
    pub show_apps: bool,
    pub show_files: bool,
    /// Search Chrome/Edge/Brave/Vivaldi bookmarks.
    pub show_bookmarks: bool,
    pub dark_mode: bool,
    pub commands: Vec<CustomCommand>,
    /// Text snippets: `name` pastes `text` (with `{date}`-style variables) into the previous window.
    pub snippets: Vec<Snippet>,
    pub web_searches: Vec<WebSearch>,
}

fn cmd(
    name: &str,
    title: &str,
    program: &str,
    args: &str,
    admin: bool,
    kw: &[&str],
) -> CustomCommand {
    CustomCommand {
        name: name.into(),
        title: title.into(),
        program: program.into(),
        args: args.into(),
        admin,
        cwd: None,
        confirm: matches!(
            name,
            "shutdown" | "restart" | "logoff" | "kill" | "emptybin" | "restartexplorer"
        ),
        keywords: kw.iter().map(|s| s.to_string()).collect(),
    }
}

fn web(key: &str, name: &str, url: &str) -> WebSearch {
    WebSearch {
        key: key.into(),
        name: name.into(),
        url: url.into(),
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            everything_path: String::new(),
            hotkey: "Alt+Space".into(),
            clipboard_hotkey: "Ctrl+Alt+V".into(),
            clipboard_enabled: true,
            clipboard_images: true,
            clipboard_max: 2000,
            clipboard_days: 90,
            clipboard_image_mb: 300,
            clipboard_ignore_apps: Vec::new(),
            visible_rows: 8,
            max_file_results: 300,
            hide_on_blur: true,
            show_recent_on_empty: false,
            window_pos: None,
            defaults_version: DEFAULTS_VERSION,
            excluded_dirs: vec![
                r"C:\$Recycle.Bin".into(),
                r"C:\Windows\WinSxS".into(),
                r"C:\Windows\SoftwareDistribution".into(),
            ],
            show_settings: true,
            show_apps: true,
            show_bookmarks: true,
            snippets: default_snippets(),
            show_files: true,
            dark_mode: true,
            commands: vec![
                cmd(
                    "cmd",
                    "命令提示符 (管理员)",
                    "cmd.exe",
                    "",
                    true,
                    &["命令行", "终端", "terminal", "command prompt"],
                ),
                cmd(
                    "ps",
                    "PowerShell (管理员)",
                    "powershell.exe",
                    "",
                    true,
                    &["powershell", "终端"],
                ),
                cmd(
                    "wt",
                    "Windows 终端 (管理员)",
                    "powershell.exe",
                    "-NoProfile -WindowStyle Hidden -Command Start-Process wt.exe -Verb RunAs",
                    false,
                    &["terminal", "终端", "windows terminal"],
                ),
                cmd(
                    "hosts",
                    "编辑 hosts 文件 (管理员)",
                    "notepad.exe",
                    r"C:\Windows\System32\drivers\etc\hosts",
                    true,
                    &["hosts", "dns"],
                ),
                cmd(
                    "reg",
                    "注册表编辑器 (管理员)",
                    "regedit.exe",
                    "",
                    true,
                    &["regedit", "注册表"],
                ),
                cmd(
                    "lock",
                    "锁定电脑",
                    "rundll32.exe",
                    "user32.dll,LockWorkStation",
                    false,
                    &["锁屏", "lock"],
                ),
                cmd(
                    "sleep",
                    "睡眠",
                    "rundll32.exe",
                    "powrprof.dll,SetSuspendState 0,1,0",
                    false,
                    &["睡眠", "suspend"],
                ),
                cmd(
                    "shutdown",
                    "关机",
                    "shutdown.exe",
                    "/s /t 0",
                    false,
                    &["关机", "power off"],
                ),
                cmd(
                    "restart",
                    "重启",
                    "shutdown.exe",
                    "/r /t 0",
                    false,
                    &["重启", "reboot"],
                ),
                cmd(
                    "logoff",
                    "注销",
                    "shutdown.exe",
                    "/l",
                    false,
                    &["注销", "sign out"],
                ),
                cmd(
                    "flushdns",
                    "刷新 DNS 缓存",
                    "cmd.exe",
                    "/k ipconfig /flushdns",
                    true,
                    &["dns"],
                ),
                cmd(
                    "ip",
                    "查看 IP 配置",
                    "cmd.exe",
                    "/k ipconfig /all",
                    false,
                    &["ipconfig", "网络"],
                ),
                cmd("ping", "Ping 主机", "cmd.exe", "/k ping {q}", false, &[]),
                cmd(
                    "kill",
                    "结束进程 (按名称)",
                    "taskkill.exe",
                    "/f /im {q}",
                    true,
                    &["taskkill", "结束进程"],
                ),
                cmd(
                    "restartexplorer",
                    "重启资源管理器",
                    "cmd.exe",
                    "/c taskkill /f /im explorer.exe & start explorer.exe",
                    false,
                    &["explorer", "资源管理器"],
                ),
                cmd(
                    "hibernate",
                    "休眠",
                    "shutdown.exe",
                    "/h",
                    false,
                    &["休眠", "hibernate"],
                ),
                cmd(
                    "shot",
                    "截图 (区域)",
                    "explorer.exe",
                    "ms-screenclip:",
                    false,
                    &["screenshot", "截图", "snip"],
                ),
                cmd(
                    "darkmode",
                    "切换深色/浅色模式",
                    "powershell.exe",
                    r"-NoProfile -WindowStyle Hidden -Command $p='HKCU:\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize'; $n=1-(Get-ItemProperty $p).AppsUseLightTheme; Set-ItemProperty $p AppsUseLightTheme $n; Set-ItemProperty $p SystemUsesLightTheme $n",
                    false,
                    &["dark", "light", "深色", "浅色", "主题", "theme"],
                ),
                cmd(
                    "emptybin",
                    "清空回收站",
                    "powershell.exe",
                    "-NoProfile -Command Clear-RecycleBin -Force",
                    false,
                    &["回收站", "recycle"],
                ),
            ],
            web_searches: vec![
                web("g", "Google", "https://www.google.com/search?q={q}"),
                web("bd", "百度", "https://www.baidu.com/s?wd={q}"),
                web("bing", "Bing", "https://www.bing.com/search?q={q}"),
                web("gh", "GitHub", "https://github.com/search?q={q}"),
                web(
                    "yt",
                    "YouTube",
                    "https://www.youtube.com/results?search_query={q}",
                ),
                web("zh", "知乎", "https://www.zhihu.com/search?q={q}"),
            ],
        }
    }
}

/// Where config, learning data and the clipboard database live: a `data` folder **next to the
/// program** (portable). If that folder is not writable (e.g. installed in Program Files) it
/// falls back to `%APPDATA%\\SwiftFind`. `SWIFTFIND_DATA` overrides both. Tests use a temp dir.
pub fn data_dir() -> PathBuf {
    static DIR: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    DIR.get_or_init(resolve_data_dir).clone()
}

/// Write via a temp file + rename so a crash or power loss can never leave a half-written file.
pub fn write_atomic(path: &std::path::Path, bytes: &[u8]) -> bool {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    std::fs::write(&tmp, bytes).is_ok() && std::fs::rename(&tmp, path).is_ok()
}

/// Move an unreadable data file aside as `<name>.bad` instead of silently overwriting it later,
/// so a typo in a hand-edited config is never destroyed.
pub fn quarantine(path: &std::path::Path) {
    let mut bad = path.as_os_str().to_owned();
    bad.push(".bad");
    if std::fs::rename(path, &bad).is_ok() {
        crate::log::info(&format!("{} was unreadable; kept as {}", path.display(), PathBuf::from(&bad).display()));
    }
}

fn writable(dir: &std::path::Path) -> bool {
    if std::fs::create_dir_all(dir).is_err() {
        return false;
    }
    let probe = dir.join(".write_test");
    let ok = std::fs::write(&probe, b"x").is_ok();
    let _ = std::fs::remove_file(&probe);
    ok
}

fn resolve_data_dir() -> PathBuf {
    if let Some(p) = std::env::var_os("SWIFTFIND_DATA") {
        return PathBuf::from(p);
    }
    if cfg!(test) {
        return std::env::temp_dir().join(format!("swiftfind_test_{}", std::process::id()));
    }
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(|p| p.join("data")))
    {
        if writable(&dir) {
            return dir;
        }
    }
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("SwiftFind")
}

fn zero() -> u32 {
    0
}

/// Bump when new built-in commands are added; `migrate` hands them to existing configs once.
const DEFAULTS_VERSION: u32 = 2;
/// Commands introduced by each version (index 0 = version 1).
const NEW_COMMANDS: &[&[&str]] = &[&["hibernate", "shot", "darkmode"], &[]];
/// Snippets introduced by each version (index 0 = version 1).
const NEW_SNIPPETS: &[&[&str]] = &[&[], &["日期", "时间"]];

impl Config {
    /// Give an older config the built-in commands added since it was written — once, and only
    /// those that are missing, so commands you deleted yourself stay deleted. Returns true when
    /// something changed (the caller saves).
    pub fn migrate(&mut self) -> bool {
        if self.defaults_version >= DEFAULTS_VERSION {
            return false;
        }
        let defaults = Config::default().commands;
        for names in NEW_COMMANDS.iter().skip(self.defaults_version as usize) {
            for name in *names {
                if !self.commands.iter().any(|c| c.name.eq_ignore_ascii_case(name)) {
                    if let Some(d) = defaults.iter().find(|c| c.name == *name) {
                        self.commands.push(d.clone());
                    }
                }
            }
        }
        let default_snippets = default_snippets();
        for names in NEW_SNIPPETS.iter().skip(self.defaults_version as usize) {
            for name in *names {
                if !self.snippets.iter().any(|s| s.name == *name) {
                    if let Some(d) = default_snippets.iter().find(|s| s.name == *name) {
                        self.snippets.push(d.clone());
                    }
                }
            }
        }
        self.defaults_version = DEFAULTS_VERSION;
        true
    }

    pub fn path() -> PathBuf {
        data_dir().join("config.json")
    }

    pub fn load() -> Self {
        Self::load_from(&Self::path())
    }

    pub fn load_from(path: &std::path::Path) -> Self {
        match std::fs::read(path) {
            Ok(b) => {
                let mut c: Config = serde_json::from_slice(&b).unwrap_or_else(|e| {
                    crate::log::info(&format!("config.json is invalid ({e}); using defaults"));
                    quarantine(path);
                    Self::default()
                });
                if c.migrate() {
                    c.save_to(path);
                }
                c
            }
            Err(_) => {
                let c = Self::default();
                c.save_to(path);
                c
            }
        }
    }

    pub fn save(&self) {
        self.save_to(&Self::path());
    }

    pub fn save_to(&self, path: &std::path::Path) {
        if let Ok(s) = serde_json::to_string_pretty(self) {
            write_atomic(path, s.as_bytes());
        }
    }

    pub fn is_excluded(&self, path: &str) -> bool {
        let p = path.to_ascii_lowercase();
        self.excluded_dirs.iter().any(|d| {
            let d = d.trim().trim_end_matches('\\').to_ascii_lowercase();
            !d.is_empty()
                && p.starts_with(&d)
                && (p.len() == d.len() || p.as_bytes().get(d.len()) == Some(&b'\\'))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exclusion_matches_dir_boundaries() {
        let c = Config {
            excluded_dirs: vec![r"D:\Games".into()],
            ..Config::default()
        };
        assert!(c.is_excluded(r"D:\Games\foo.exe"));
        assert!(c.is_excluded(r"d:\games"));
        assert!(!c.is_excluded(r"D:\GamesBackup\foo.exe"));
    }

    #[test]
    fn dangerous_default_commands_require_confirmation() {
        let c = Config::default();
        for name in ["shutdown", "restart", "logoff", "kill", "emptybin"] {
            assert!(
                c.commands.iter().find(|x| x.name == name).unwrap().confirm,
                "{name}"
            );
        }
        assert!(!c.commands.iter().find(|x| x.name == "cmd").unwrap().confirm);
        assert_eq!(c.clipboard_hotkey, "Ctrl+Alt+V");
    }

    #[test]
    fn partial_config_uses_defaults() {
        let c: Config = serde_json::from_str(r#"{"hotkey":"Ctrl+Space"}"#).unwrap();
        assert_eq!(c.hotkey, "Ctrl+Space");
        assert!(!c.commands.is_empty());
    }

    #[test]
    fn broken_config_is_kept_aside_not_overwritten() {
        let dir = std::env::temp_dir().join(format!("sf_cfg_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let p = dir.join("config.json");
        std::fs::write(&p, "{ \"hotkey\": \"Alt+Space\", oops").unwrap();
        let c = Config::load_from(&p);
        assert_eq!(c.hotkey, Config::default().hotkey);
        assert!(dir.join("config.json.bad").exists(), "user's file must be preserved");
        assert!(!p.exists());
        // Missing file: defaults are written, and a partial file keeps unspecified defaults.
        let c2 = Config::load_from(&p);
        assert!(p.exists() && c2.hotkey == c.hotkey);
        std::fs::write(&p, r#"{ "hotkey": "Ctrl+Alt+K" }"#).unwrap();
        let c3 = Config::load_from(&p);
        assert_eq!(c3.hotkey, "Ctrl+Alt+K");
        assert!(!c3.commands.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn migration_adds_new_default_commands_once_and_respects_deletions() {
        let mut c = Config { defaults_version: 0, ..Config::default() };
        c.commands.retain(|x| !["darkmode", "ping"].contains(&x.name.as_str()));
        assert!(c.migrate());
        assert!(c.commands.iter().any(|x| x.name == "darkmode"), "new command delivered");
        assert!(!c.commands.iter().any(|x| x.name == "ping"), "user's deletion respected");
        assert_eq!(c.defaults_version, DEFAULTS_VERSION);
        let n = c.commands.len();
        c.commands.retain(|x| x.name != "darkmode");
        assert!(!c.migrate(), "runs only once");
        assert_eq!(c.commands.len(), n - 1);
        // A config file from an older version (no version field) triggers it on load.
        let dir = std::env::temp_dir().join(format!("sf_mig_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let p = dir.join("config.json");
        std::fs::write(&p, r#"{ "commands": [] }"#).unwrap();
        let loaded = Config::load_from(&p);
        assert_eq!(loaded.commands.len(), 3);
        assert!(std::fs::read_to_string(&p).unwrap().contains("defaults_version"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
