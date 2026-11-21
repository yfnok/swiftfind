//! Search engine: merges local sources (apps, settings, commands, calculator, web) with
//! Everything file hits, applies learned boosts and returns one ranked list.

use crate::apps::AppEntry;
use crate::clipboard;
use crate::clipdb::{ClipKind, Store};
use crate::config::Config;
use crate::everything::FileHit;
use crate::learn::{normalize_query, Learner};
use crate::matcher::{self, Haystack};
use crate::settings_db::SETTINGS;
use crate::pathnav::{self, NavEntry, NAV_SCORE_BASE};
use crate::recent::{self, Recent};
use crate::types::{Action, Item, Kind};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub const MAX_RESULTS: usize = 60;

struct Local {
    item: Item,
    hay: Haystack,
    /// Exact trigger word (commands only) – typing exactly this puts the item first.
    trigger: Option<String>,
}

pub struct Engine {
    pub cfg: Config,
    pub learner: Learner,
    /// Clipboard history (shared database handle; in-memory until the UI attaches
    /// the real database).
    pub clip: Store,
    locals: Vec<Local>,
    /// Files/folders the user launched before: always candidates, even when Everything's
    /// (limited) result window does not contain them.
    learned: Vec<Local>,
    learned_ids: HashSet<String>,
    /// Last folder listing for path browsing (re-reading a big folder on every key is wasteful).
    nav_cache: Mutex<Option<(PathBuf, Instant, Vec<NavEntry>)>>,
    recent_cache: Mutex<Option<(Instant, Vec<Recent>)>>,
}

/// Subtitle of the "open this folder" row shown first when a folder path is typed in full.
pub const OPEN_DIR_SUBTITLE: &str = "打开此文件夹";

pub fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn is_uri(t: &str) -> bool {
    let l = t.to_ascii_lowercase();
    l.starts_with("ms-settings:")
        || l.starts_with("windowsdefender:")
        || l.starts_with("shell:")
        || l.starts_with("http")
}

fn target_action(target: &str, admin: bool) -> Action {
    if let Some(rest) = target.strip_prefix("cmd:") {
        let (prog, args) = rest.split_once('|').unwrap_or((rest, ""));
        return Action::Open {
            target: prog.into(),
            args: args.into(),
            admin,
            cwd: None,
        };
    }
    if is_uri(target) {
        return Action::Open {
            target: target.into(),
            args: String::new(),
            admin,
            cwd: None,
        };
    }
    let (prog, args) = target.split_once(' ').unwrap_or((target, ""));
    Action::Open {
        target: prog.into(),
        args: args.into(),
        admin,
        cwd: None,
    }
}

fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

fn basename(path: &str) -> &str {
    path.rsplit(['\\', '/']).next().unwrap_or(path)
}

/// Base ranking for a file/folder hit: filename quality, then shallow-path and location priors.
fn file_base_score(path: &str, lp: &str, is_folder: bool, tokens: &[&str]) -> f32 {
    let name = basename(path);
    let hay = Haystack::new(name, "");
    let mut s = match matcher::score(&hay, tokens) {
        Some(m) => 20.0 + m * 0.45, // name hit: 20..65
        None => 8.0,                // matched on path only
    };
    let ext = name
        .rsplit('.')
        .next()
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    if !is_folder && matches!(ext.as_str(), "exe" | "lnk" | "msc" | "bat" | "cmd") {
        s += 6.0;
    }
    let depth = path.matches('\\').count() as f32;
    s -= (depth * 0.6).min(8.0);
    if lp.contains(r"\appdata\") || lp.contains(r"\node_modules\") || lp.contains(r"\windows\") {
        s -= 6.0;
    }
    s
}

/// How good a file match is, 0..=1, from `file_base_score` (range roughly 8..65).
fn file_quality(score: f32) -> f32 {
    ((score - 8.0) / 57.0).clamp(0.0, 1.0)
}

fn parent(path: &str) -> &str {
    match path.rfind(['\\', '/']) {
        Some(i) if i > 0 => &path[..i],
        _ => path,
    }
}

impl Engine {
    pub fn new(cfg: Config, learner: Learner) -> Self {
        let clip = Store::open_memory().expect("in-memory db");
        let mut e = Self {
            cfg,
            learner,
            clip,
            locals: Vec::new(),
            learned: Vec::new(),
            learned_ids: HashSet::new(),
            nav_cache: Mutex::new(None),
            recent_cache: Mutex::new(None),
        };
        e.rebuild(&[]);
        e
    }

    fn rebuild_learned(&mut self) {
        let mut v = Vec::new();
        let mut ids = HashSet::new();
        for (id, path) in self.learner.paths() {
            let name = basename(path);
            v.push(Local {
                item: Item {
                    id: id.to_string(),
                    title: name.to_string(),
                    subtitle: parent(path).to_string(),
                    kind: Kind::File,
                    action: Action::Open {
                        target: path.to_string(),
                        args: String::new(),
                        admin: false,
                        cwd: None,
                    },
                    path: Some(path.to_string()),
                    score: 0.0,
                },
                hay: Haystack::new(name, &crate::py::keywords(name)),
                trigger: None,
            });
            ids.insert(id.to_string());
        }
        self.learned = v;
        self.learned_ids = ids;
    }

    pub fn set_clip_store(&mut self, store: Store) {
        self.clip = store;
    }

    /// `(row id, is_image)` for clipboard-history items (`clip:t12` / `clip:i12`).
    pub fn clip_id(item: &Item) -> Option<(i64, bool)> {
        let rest = item.id.strip_prefix("clip:")?;
        let image = rest.starts_with('i');
        rest[1..].parse().ok().map(|id| (id, image))
    }

    pub fn toggle_pin(&self, item: &Item) -> Option<bool> {
        Self::clip_id(item).and_then(|(id, _)| self.clip.toggle_pin(id))
    }

    /// `cb <text>` / `clip <text>` lists clipboard history: newest first, pinned on top.
    /// Extra words filter by content or source program; `img`, `text` and `pin` filter by type.
    fn clipboard_view(&self, q: &str) -> Option<Vec<Item>> {
        if !self.cfg.clipboard_enabled {
            return None;
        }
        let (first, rest) = match q.split_once(char::is_whitespace) {
            Some((f, r)) => (f, r.trim()),
            None => (q, ""),
        };
        if !(first.eq_ignore_ascii_case("cb") || first.eq_ignore_ascii_case("clip")) {
            return None;
        }
        let now = clipboard::now_ms();
        let rows = self.clip.search(rest, MAX_RESULTS);
        Some(
            rows.into_iter()
                .enumerate()
                .map(|(i, r)| {
                    let when = clipboard::ago(now, r.ts_ms);
                    let src = if r.src.is_empty() {
                        String::new()
                    } else {
                        format!(" · {}", r.src)
                    };
                    let star = if r.pinned { "★ " } else { "" };
                    let (id, title, subtitle, image) = match r.kind {
                        ClipKind::Text => {
                            let lines = if r.lines > 1 {
                                format!(" · {} 行", r.lines)
                            } else {
                                String::new()
                            };
                            (
                                format!("clip:t{}", r.id),
                                format!("{star}{}", clipboard::preview(&r.preview, 100)),
                                format!("{} 字符{lines} · {when}{src}", r.chars),
                                false,
                            )
                        }
                        ClipKind::Image => (
                            format!("clip:i{}", r.id),
                            format!("{star}图片 {}×{}", r.w, r.h),
                            format!("{} · {when}{src}", crate::clipimg::human_bytes(r.bytes)),
                            true,
                        ),
                    };
                    Item {
                        id,
                        title,
                        subtitle,
                        kind: Kind::Clip,
                        action: Action::PasteClip { id: r.id, image },
                        path: None,
                        score: 1000.0 - i as f32,
                    }
                })
                .collect(),
        )
    }

    /// Rebuild the local index (call after config changes or a fresh app scan).
    pub fn rebuild(&mut self, apps: &[AppEntry]) {
        let mut v: Vec<Local> =
            Vec::with_capacity(SETTINGS.len() + apps.len() + self.cfg.commands.len());

        if self.cfg.show_settings {
            for d in SETTINGS {
                let kw = format!("{} {}", d.en, d.kw);
                v.push(Local {
                    item: Item {
                        id: format!("setting:{}|{}", d.target, d.zh),
                        title: d.zh.to_string(),
                        subtitle: d.en.to_string(),
                        kind: Kind::Setting,
                        action: target_action(d.target, d.admin),
                        path: None,
                        score: 0.0,
                    },
                    hay: Haystack::new(d.zh, &kw),
                    trigger: None,
                });
            }
        }

        if self.cfg.show_apps {
            for a in apps {
                let id = if a.uwp {
                    format!("app:{}", a.target.to_lowercase())
                } else {
                    a.target.to_lowercase()
                };
                v.push(Local {
                    item: Item {
                        id,
                        title: a.name.clone(),
                        subtitle: if a.uwp {
                            "Microsoft Store 应用".into()
                        } else {
                            a.target.clone()
                        },
                        kind: Kind::App,
                        action: Action::Open {
                            target: a.target.clone(),
                            args: String::new(),
                            admin: false,
                            cwd: None,
                        },
                        path: if a.uwp { None } else { Some(a.target.clone()) },
                        score: 0.0,
                    },
                    hay: Haystack::new(&a.name, &crate::py::keywords(&a.name)),
                    trigger: None,
                });
            }
        }

        for c in &self.cfg.commands {
            let kw = format!("{} {}", c.name, c.keywords.join(" "));
            let sub = if c.args.is_empty() {
                c.program.clone()
            } else {
                format!("{} {}", c.program, c.args)
            };
            v.push(Local {
                item: Item {
                    id: format!("cmd:{}", c.name.to_lowercase()),
                    title: c.title.clone(),
                    subtitle: format!("{}{}", if c.admin { "管理员 · " } else { "" }, sub),
                    kind: Kind::Command,
                    action: Action::Open {
                        target: c.program.clone(),
                        args: c.args.clone(),
                        admin: c.admin,
                        cwd: c.cwd.clone(),
                    },
                    path: None,
                    score: 0.0,
                },
                hay: Haystack::new(&c.title, &format!("{kw} {}", crate::py::keywords(&c.title))),
                trigger: Some(c.name.to_lowercase()),
            });
        }

        v.push(Local {
            item: Item {
                id: "builtin:settings".into(),
                title: "SwiftFind 设置".into(),
                subtitle: "配置 Everything 路径、快捷键、排除目录、自定义命令".into(),
                kind: Kind::Command,
                action: Action::OpenPanel,
                path: None,
                score: 0.0,
            },
            hay: Haystack::new(
                "SwiftFind 设置",
                "swiftfind settings preferences config 配置 选项",
            ),
            trigger: Some("sf".into()),
        });
        v.push(Local {
            item: Item {
                id: "builtin:quit".into(),
                title: "退出 SwiftFind".into(),
                subtitle: "关闭后台程序".into(),
                kind: Kind::Command,
                action: Action::Quit,
                path: None,
                score: 0.0,
            },
            hay: Haystack::new("退出 SwiftFind", "quit exit close 关闭"),
            trigger: Some("quit".into()),
        });
        self.locals = v;
        self.rebuild_learned();
    }

    /// Items the learner considers most useful, shown when the query is empty.
    fn frequent(&self, now: u64) -> Vec<Item> {
        let top: Vec<(String, f32)> = self
            .learner
            .top(12, now)
            .into_iter()
            .map(|(k, s)| (k.to_string(), s))
            .collect();
        let mut out = Vec::new();
        for (id, s) in top {
            if let Some(l) = self.locals.iter().find(|l| l.item.id == id) {
                let mut it = l.item.clone();
                it.score = s;
                out.push(it);
            } else if !id.starts_with("setting:")
                && !id.starts_with("app:")
                && !id.starts_with("cmd:")
                && !id.starts_with("builtin:")
            {
                // A learned file/folder path (stored lowercase). Show only if it still exists.
                let p = std::path::Path::new(&id);
                if p.exists() && !self.cfg.is_excluded(&id) {
                    let is_dir = p.is_dir();
                    out.push(Item {
                        id: id.clone(),
                        title: basename(&id).to_string(),
                        subtitle: parent(&id).to_string(),
                        kind: if is_dir { Kind::Folder } else { Kind::File },
                        action: Action::Open {
                            target: id.clone(),
                            args: String::new(),
                            admin: false,
                            cwd: None,
                        },
                        path: Some(id.clone()),
                        score: s,
                    });
                }
            }
        }
        out
    }

    /// Full ranking. `files` are Everything hits; `files_fresh` says whether they belong to `query`.
    /// Folder listing, cached for a couple of seconds. `None` = the folder did not answer in time.
    fn nav_entries(&self, dir: &std::path::Path) -> Option<Vec<NavEntry>> {
        if let Ok(g) = self.nav_cache.lock() {
            if let Some((d, t, v)) = g.as_ref() {
                if d == dir && t.elapsed() < Duration::from_secs(2) {
                    return Some(v.clone());
                }
            }
        }
        let v = pathnav::list(dir)?;
        if let Ok(mut g) = self.nav_cache.lock() {
            *g = Some((dir.to_path_buf(), Instant::now(), v.clone()));
        }
        Some(v)
    }

    /// Recent files from Windows' own history, newest first (cached briefly).
    fn recent_view(&self, q: &str) -> Option<Vec<Item>> {
        let filter = recent::trigger(q)?;
        let cached = self.recent_cache.lock().ok().and_then(|g| g.as_ref().filter(|(t, _)| t.elapsed() < Duration::from_secs(5)).map(|(_, v)| v.clone()));
        let list = match cached {
            Some(v) => v,
            None => {
                let v = recent::recent_dir().and_then(|d| recent::list(&d)).unwrap_or_default();
                if let Ok(mut g) = self.recent_cache.lock() {
                    *g = Some((Instant::now(), v.clone()));
                }
                v
            }
        };
        let tokens: Vec<&str> = filter.split_whitespace().collect();
        let mut out = Vec::new();
        for (n, r) in list.iter().filter(|r| !self.cfg.is_excluded(&r.path)).enumerate() {
            let lp = r.path.to_lowercase();
            if !tokens.iter().all(|t| lp.contains(t)) {
                continue;
            }
            out.push(Item {
                id: lp,
                title: basename(&r.path).to_string(),
                subtitle: parent(&r.path).to_string(),
                kind: if r.is_dir { Kind::Folder } else { Kind::File },
                action: Action::Open { target: r.path.clone(), args: String::new(), admin: false, cwd: None },
                path: Some(r.path.clone()),
                score: NAV_SCORE_BASE - n as f32,
            });
            if out.len() >= 40 {
                break;
            }
        }
        Some(out)
    }

    /// Path browsing: `C:\Users\`, `~\Docs`, `%appdata%` list that folder.
    fn path_view(&self, q: &str) -> Option<Vec<Item>> {
        let nq = pathnav::parse(q, &|n| std::env::var(n).ok())?;
        let dir_text = nq.dir.to_string_lossy().into_owned();
        let mut out: Vec<Item> = Vec::new();
        let Some(entries) = self.nav_entries(&nq.dir) else {
            out.push(Item {
                id: "nav:slow".into(),
                title: "读取此目录超时".into(),
                subtitle: dir_text,
                kind: Kind::Folder,
                action: Action::Copy(String::new()),
                path: None,
                score: NAV_SCORE_BASE,
            });
            return Some(out);
        };
        let mut n = 0f32;
        let mut push = |out: &mut Vec<Item>, id_path: &str, title: String, subtitle: String, is_dir: bool| {
            out.push(Item {
                id: id_path.to_lowercase(),
                title,
                subtitle,
                kind: if is_dir { Kind::Folder } else { Kind::File },
                action: Action::Open { target: id_path.to_string(), args: String::new(), admin: false, cwd: None },
                path: Some(id_path.to_string()),
                score: NAV_SCORE_BASE - n,
            });
            n += 1.0;
        };
        if nq.trailing_sep && nq.dir.is_dir() {
            let shown = dir_text.trim_end_matches(['\\', '/']);
            let target = if shown.ends_with(':') { dir_text.as_str() } else { shown };
            push(&mut out, target, dir_text.clone(), OPEN_DIR_SUBTITLE.to_string(), true);
        }
        for e in pathnav::select(&entries, &nq.filter) {
            let full = format!("{dir_text}{}", e.name);
            push(&mut out, &full, e.name.clone(), dir_text.clone(), e.is_dir);
        }
        Some(out)
    }

    pub fn results(&self, query: &str, files: &[FileHit], files_fresh: bool) -> Vec<Item> {
        let now = now_secs();
        let q = query.trim();
        if q.is_empty() {
            return if self.cfg.show_recent_on_empty { self.frequent(now) } else { Vec::new() };
        }
        if let Some(items) = self.path_view(q) {
            return items;
        }
        if let Some(items) = self.recent_view(q) {
            return items;
        }
        if let Some(items) = self.clipboard_view(q) {
            return items;
        }
        let qn = normalize_query(q);
        let tokens: Vec<&str> = qn.split(' ').collect();
        let mut out: Vec<Item> = Vec::with_capacity(64);

        // Calculator.
        if let Some(v) = crate::calc::eval(q) {
            out.push(Item {
                id: format!("calc:{v}"),
                title: format!("= {v}"),
                subtitle: "按 Enter 复制结果".into(),
                kind: Kind::Calc,
                action: Action::Copy(v),
                path: None,
                score: 500.0,
            });
        }

        // Trigger-word forms: "g rust", "ping 8.8.8.8".
        let (first, rest) = match q.split_once(char::is_whitespace) {
            Some((f, r)) => (f.to_lowercase(), r.trim()),
            None => (q.to_lowercase(), ""),
        };
        if !rest.is_empty() {
            if let Some(w) = self
                .cfg
                .web_searches
                .iter()
                .find(|w| w.key.eq_ignore_ascii_case(&first))
            {
                out.push(Item {
                    id: format!("web:{}", w.key),
                    title: format!("{} 搜索: {}", w.name, rest),
                    subtitle: w.url.replace("{q}", &url_encode(rest)),
                    kind: Kind::Web,
                    action: Action::Open {
                        target: w.url.replace("{q}", &url_encode(rest)),
                        args: String::new(),
                        admin: false,
                        cwd: None,
                    },
                    path: None,
                    score: 400.0,
                });
            }
            for c in self
                .cfg
                .commands
                .iter()
                .filter(|c| c.name.eq_ignore_ascii_case(&first) && c.args.contains("{q}"))
            {
                out.push(Item {
                    id: format!("cmd:{}", c.name.to_lowercase()),
                    title: format!("{} {}", c.title, rest),
                    subtitle: format!(
                        "{}{} {}",
                        if c.admin { "管理员 · " } else { "" },
                        c.program,
                        c.args.replace("{q}", rest)
                    ),
                    kind: Kind::Command,
                    action: Action::Open {
                        target: c.program.clone(),
                        args: c.args.replace("{q}", rest),
                        admin: c.admin,
                        cwd: c.cwd.clone(),
                    },
                    path: None,
                    score: 390.0,
                });
            }
        }

        // Local sources.
        for l in &self.locals {
            let Some(mut s) = matcher::score(&l.hay, &tokens) else {
                continue;
            };
            let quality = (s / 85.0).min(1.0); // text-match strength, before priors/bonuses
            if l.trigger.as_deref() == Some(qn.as_str()) {
                s += 60.0; // exact trigger word, e.g. "cmd"
            }
            // Kind priors: apps and commands slightly ahead of settings, files below.
            s += match l.item.kind {
                Kind::Command => 6.0,
                Kind::App => 4.0,
                _ => 0.0,
            };
            let mut it = l.item.clone();
            it.score = s + self.learner.boost(&it.id, &qn, now, quality);
            out.push(it);
        }

        // Files launched before: candidates regardless of Everything's result window.
        if self.cfg.show_files {
            for l in &self.learned {
                let Some(path) = l.item.path.as_deref() else {
                    continue;
                };
                if self.cfg.is_excluded(path) || matcher::score(&l.hay, &tokens).is_none() {
                    continue;
                }
                let s = file_base_score(path, &l.item.id, false, &tokens);
                let mut it = l.item.clone();
                it.score = s + self.learner.boost(&it.id, &qn, now, file_quality(s));
                out.push(it);
            }
        }

        // Files from Everything.
        if self.cfg.show_files {
            let lower_tokens: Vec<String> = tokens.iter().map(|t| t.to_string()).collect();
            for h in files {
                if self.cfg.is_excluded(&h.path) {
                    continue;
                }
                let lp = h.path.to_lowercase();
                if !files_fresh && !lower_tokens.iter().all(|t| lp.contains(t.as_str())) {
                    continue;
                }
                let name = basename(&h.path);
                let s = file_base_score(&h.path, &lp, h.is_folder, &tokens);
                let id = lp;
                let boost = self.learner.boost(&id, &qn, now, file_quality(s));
                out.push(Item {
                    id,
                    title: name.to_string(),
                    subtitle: parent(&h.path).to_string(),
                    kind: if h.is_folder {
                        Kind::Folder
                    } else {
                        Kind::File
                    },
                    action: Action::Open {
                        target: h.path.clone(),
                        args: String::new(),
                        admin: false,
                        cwd: None,
                    },
                    path: Some(h.path.clone()),
                    score: s + boost,
                });
            }
        }

        out.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let mut seen = HashSet::new();
        out.retain(|i| seen.insert(i.id.clone()));
        out.truncate(MAX_RESULTS);

        // Learned files may have been moved or deleted since: verify the ones we are about to show.
        out.retain_mut(|i| {
            if !self.learned_ids.contains(&i.id) {
                return true;
            }
            let Some(p) = i.path.as_deref() else {
                return true;
            };
            match std::fs::metadata(p) {
                Ok(m) => {
                    if m.is_dir() {
                        i.kind = Kind::Folder;
                    }
                    true
                }
                Err(_) => false,
            }
        });

        // Always offer a web search fallback at the bottom.
        if let Some(w) = self.cfg.web_searches.first() {
            if !qn.is_empty() && !out.iter().any(|i| i.kind == Kind::Calc) {
                let url = w.url.replace("{q}", &url_encode(q));
                out.push(Item {
                    id: format!("web:{}:{}", w.key, qn),
                    title: format!("用 {} 搜索 “{}”", w.name, q),
                    subtitle: url.clone(),
                    kind: Kind::Web,
                    action: Action::Open {
                        target: url,
                        args: String::new(),
                        admin: false,
                        cwd: None,
                    },
                    path: None,
                    score: 0.0,
                });
            }
        }
        out
    }

    /// Record a launch so the item ranks higher next time. `skipped` are ids that were listed
    /// above the chosen item for this query; they sink slightly for the same query.
    pub fn learn(&mut self, item: &Item, query: &str, skipped: &[String]) {
        if matches!(item.kind, Kind::Calc | Kind::Clip) || item.score >= NAV_SCORE_BASE
            || item.id.starts_with("web:") && item.score <= 0.0
        {
            return;
        }
        let now = now_secs();
        let path = if matches!(item.kind, Kind::File | Kind::Folder) {
            item.path.as_deref()
        } else {
            None
        };
        self.learner.record_with_path(&item.id, query, now, path);
        for id in skipped {
            if *id != item.id {
                self.learner.record_skip(id, query, now);
            }
        }
        self.learner.save_if_dirty();
        if path.is_some() {
            self.rebuild_learned();
        }
    }

    pub fn forget(&mut self, item: &Item) {
        if let Some((id, _)) = Self::clip_id(item) {
            self.clip.delete(id);
            return;
        }
        self.learner.forget(&item.id);
        self.learner.save_if_dirty();
        self.rebuild_learned();
    }

    pub fn exclude(&mut self, path: &str) {
        if !self
            .cfg
            .excluded_dirs
            .iter()
            .any(|d| d.eq_ignore_ascii_case(path))
        {
            self.cfg.excluded_dirs.push(path.to_string());
            self.cfg.save();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn engine() -> Engine {
        let mut e = Engine::new(Config::default(), Learner::in_memory());
        e.rebuild(&[
            AppEntry {
                name: "Visual Studio Code".into(),
                target: r"C:\sm\Visual Studio Code.lnk".into(),
                uwp: false,
            },
            AppEntry {
                name: "Visual Studio 2022".into(),
                target: r"C:\sm\Visual Studio 2022.lnk".into(),
                uwp: false,
            },
        ]);
        e
    }

    #[test]
    fn path_browsing_lists_filters_and_is_not_learned() {
        let dir = std::env::temp_dir().join(format!("sf_engnav_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("Projects")).unwrap();
        std::fs::write(dir.join("notes.txt"), b"x").unwrap();
        std::env::set_var("SF_NAV_TEST_DIR", &dir);
        let mut e = engine();
        let all = e.results("%SF_NAV_TEST_DIR%/", &[], true);
        let titles: Vec<_> = all.iter().map(|i| i.title.as_str()).collect();
        assert_eq!(all[0].subtitle, OPEN_DIR_SUBTITLE, "{titles:?}");
        assert_eq!(&titles[1..], ["Projects", "notes.txt"]);
        assert_eq!(all[1].kind, Kind::Folder);
        let some = e.results("%SF_NAV_TEST_DIR%/no", &[], true);
        assert_eq!(some.len(), 1);
        assert_eq!(some[0].title, "notes.txt");
        // Browsing never pollutes the learned list.
        let n = e.learner.len();
        e.learn(&some[0], "%SF_NAV_TEST_DIR%/no", &[]);
        assert_eq!(e.learner.len(), n);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn chinese_app_names_found_by_pinyin() {
        let mut e = engine();
        e.rebuild(&[
            AppEntry { name: "微信".into(), target: r"C:\sm\微信.lnk".into(), uwp: false },
            AppEntry { name: "网易云音乐".into(), target: r"C:\sm\网易云音乐.lnk".into(), uwp: false },
            AppEntry { name: "腾讯会议".into(), target: r"C:\sm\腾讯会议.lnk".into(), uwp: false },
        ]);
        for (q, want) in [("wx", "微信"), ("weixin", "微信"), ("wei", "微信"), ("wyyyy", "网易云音乐"), ("wangyiyunyinyue", "网易云音乐"), ("txhy", "腾讯会议"), ("微", "微信")] {
            let r = e.results(q, &[], true);
            assert!(r.iter().take(3).any(|i| i.title == want), "{q}: {:?}", r.iter().map(|i| &i.title).collect::<Vec<_>>());
        }
    }

    #[test]
    fn env_vars_found_by_chinese_english_and_pinyin() {
        let e = engine();
        for q in ["环境变量", "environment", "hjbl", "env"] {
            let r = e.results(q, &[], true);
            assert_eq!(
                r[0].title,
                "环境变量",
                "query {q}: {:?}",
                r.iter().map(|i| &i.title).collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn firewall_and_cmd_admin() {
        let e = engine();
        let r = e.results("firewall", &[], true);
        assert!(r.iter().any(|i| i.title.contains("防火墙")));
        let r = e.results("cmd", &[], true);
        assert_eq!(r[0].kind, Kind::Command);
        match &r[0].action {
            Action::Open { admin, .. } => assert!(*admin),
            _ => panic!(),
        }
    }

    #[test]
    fn learning_promotes_item() {
        let mut e = engine();
        let before = e.results("vis", &[], true);
        // Pick whichever app is NOT currently on top; after learning it must take the top spot.
        let loser = before
            .iter()
            .find(|i| i.kind == Kind::App && i.id != before[0].id)
            .unwrap()
            .clone();
        e.learn(&loser, "vis", &[]);
        let after = e.results("vis", &[], true);
        assert_eq!(after[0].id, loser.id);
        // And the next-most-used item moves up behind it ("后面依次排到前面").
        let third = after
            .iter()
            .find(|i| i.kind == Kind::App && i.id != loser.id)
            .unwrap()
            .clone();
        e.learn(&third, "vis", &[]);
        e.learn(&third, "vis", &[]);
        assert_eq!(e.results("vis", &[], true)[0].id, third.id);
    }

    #[test]
    fn calc_web_and_files() {
        let e = engine();
        assert_eq!(e.results("2*21", &[], true)[0].title, "= 42");
        assert_eq!(e.results("g rust lang", &[], true)[0].kind, Kind::Web);
        let hits = vec![
            FileHit {
                path: r"D:\docs\report.docx".into(),
                is_folder: false,
            },
            FileHit {
                path: r"D:\Games\report.docx".into(),
                is_folder: false,
            },
        ];
        let mut e2 = engine();
        e2.cfg.excluded_dirs = vec![r"D:\Games".into()];
        let r = e2.results("report", &hits, true);
        assert_eq!(r.iter().filter(|i| i.kind == Kind::File).count(), 1);
    }

    #[test]
    fn stale_files_are_filtered() {
        let e = engine();
        let hits = vec![FileHit {
            path: r"D:\a\alpha.txt".into(),
            is_folder: false,
        }];
        assert!(e
            .results("beta", &hits, false)
            .iter()
            .all(|i| i.kind != Kind::File));
        assert!(e
            .results("alp", &hits, false)
            .iter()
            .any(|i| i.kind == Kind::File));
    }

    #[test]
    fn empty_query_shows_frequent_only_when_enabled() {
        let mut e = engine();
        assert!(e.results("", &[], true).is_empty(), "clean search bar by default");
        e.cfg.show_recent_on_empty = true;
        let r = e.results("vis", &[], true);
        e.learn(&r[0], "vis", &[]);
        let f = e.results("", &[], true);
        assert_eq!(f[0].id, r[0].id);
    }

    #[test]
    fn clipboard_view_lists_filters_and_pins() {
        let e = engine();
        e.clip
            .add_text("hello world", "notepad.exe", 1_000)
            .unwrap();
        e.clip.add_text("rust 剪贴板", "code.exe", 2_000).unwrap();
        e.clip
            .add_image(&[1, 2, 3], &[4], 640, 480, "snip.exe", 3_000)
            .unwrap();
        let r = e.results("cb", &[], true);
        assert_eq!(r.len(), 3);
        assert_eq!(r[0].kind, Kind::Clip);
        assert!(r[0].title.starts_with("图片 640×480"), "{}", r[0].title);
        assert!(matches!(r[0].action, Action::PasteClip { image: true, .. }));
        assert!(matches!(
            r[1].action,
            Action::PasteClip { image: false, .. }
        ));
        assert!(r[1].subtitle.contains("code.exe"));
        assert_eq!(e.results("cb HELLO", &[], true).len(), 1);
        assert_eq!(e.results("cb notepad", &[], true).len(), 1); // by source program
        assert_eq!(e.results("cb img", &[], true).len(), 1);
        assert!(e
            .results("cbx", &[], true)
            .iter()
            .all(|i| i.kind != Kind::Clip));
        // Pin the oldest one: it moves to the top and is starred.
        let oldest = r.last().unwrap().clone();
        assert_eq!(e.toggle_pin(&oldest), Some(true));
        let r2 = e.results("cb", &[], true);
        assert_eq!(r2[0].id, oldest.id);
        assert!(r2[0].title.starts_with("★ "));
        assert_eq!(Engine::clip_id(&oldest).map(|(_, img)| img), Some(false));
    }

    #[test]
    fn forgetting_a_clip_deletes_it() {
        let mut e = engine();
        e.clip.add_text("secret note", "", 1).unwrap();
        let it = e.results("cb", &[], true)[0].clone();
        e.forget(&it);
        assert!(e.results("cb", &[], true).is_empty());
    }

    #[test]
    fn skipped_item_loses_rank_for_that_query() {
        let mut e = engine();
        let before = e.results("vis", &[], true);
        let (a, b) = (before[0].clone(), before[1].clone());
        e.learn(&b, "vis", std::slice::from_ref(&a.id));
        let after = e.results("vis", &[], true);
        assert_eq!(after[0].id, b.id);
        let sa = after.iter().find(|i| i.id == a.id).unwrap().score;
        assert!(
            sa < a.score,
            "skipped item should score lower: {sa} vs {}",
            a.score
        );
    }

    #[test]
    fn learned_files_surface_without_everything_and_vanish_when_deleted() {
        let dir = std::env::temp_dir().join(format!("sf_eng_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("Quarterly Report.txt");
        std::fs::write(&f, "x").unwrap();
        let path = f.to_string_lossy().to_string();
        let mut e = engine();
        let item = Item {
            id: path.to_lowercase(),
            title: "Quarterly Report.txt".into(),
            subtitle: String::new(),
            kind: Kind::File,
            action: Action::Open {
                target: path.clone(),
                args: String::new(),
                admin: false,
                cwd: None,
            },
            path: Some(path.clone()),
            score: 0.0,
        };
        e.learn(&item, "qr", &[]);
        let r = e.results("quarterly", &[], true);
        assert!(r.iter().any(|i| i.path.as_deref() == Some(path.as_str())));
        std::fs::remove_file(&f).unwrap();
        assert!(e
            .results("quarterly", &[], true)
            .iter()
            .all(|i| i.kind != Kind::File));
        let _ = std::fs::remove_dir(&dir);
    }
}
