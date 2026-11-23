//! egui front end: search box, result list, settings panel.
#![allow(deprecated)] // egui::Id::new is deprecated in 0.37 but remains the simplest stable-id API

use crate::apps::{self, AppEntry};
use crate::clipboard::{self, ClipSettings};
use crate::clipdb::{self, ClipContent};
use crate::clipimg;
use crate::config::{self, Config};
use crate::engine::Engine;
use crate::everything::{self, EvError, FileHit, SearchReply, SearchRequest};
use crate::learn::Learner;
use crate::platform::{self, Shared};
use crate::types::{Action, Item, Kind};
use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontId, Key, Modifiers, Pos2, Rect, Sense, Stroke,
    StrokeKind, Vec2,
};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const WIDTH: f32 = 760.0;
const HEADER_H: f32 = 64.0;
const ROW_H: f32 = 52.0;
const FOOTER_H: f32 = 30.0;
const PAD: f32 = 10.0;
const PANEL_H: f32 = 540.0;
const RESCAN_EVERY: Duration = Duration::from_secs(15 * 60);

#[derive(Clone, Copy)]
struct Palette {
    bg: Color32,
    border: Color32,
    text: Color32,
    dim: Color32,
    sel: Color32,
    hover: Color32,
    accent: Color32,
    input_bg: Color32,
}

impl Palette {
    fn dark() -> Self {
        Self {
            bg: Color32::from_rgb(25, 27, 33),
            border: Color32::from_rgb(58, 62, 74),
            text: Color32::from_rgb(232, 234, 240),
            dim: Color32::from_rgb(133, 139, 154),
            sel: Color32::from_rgb(46, 60, 94),
            hover: Color32::from_rgb(36, 40, 50),
            accent: Color32::from_rgb(96, 150, 255),
            input_bg: Color32::from_rgb(34, 37, 46),
        }
    }
    fn light() -> Self {
        Self {
            bg: Color32::from_rgb(250, 250, 252),
            border: Color32::from_rgb(214, 217, 226),
            text: Color32::from_rgb(28, 30, 38),
            dim: Color32::from_rgb(120, 126, 140),
            sel: Color32::from_rgb(220, 232, 255),
            hover: Color32::from_rgb(238, 241, 248),
            accent: Color32::from_rgb(56, 118, 240),
            input_bg: Color32::from_rgb(240, 242, 247),
        }
    }
}

fn kind_color(k: Kind) -> Color32 {
    match k {
        Kind::Command => Color32::from_rgb(255, 159, 67),
        Kind::Setting => Color32::from_rgb(38, 198, 170),
        Kind::App => Color32::from_rgb(96, 150, 255),
        Kind::File => Color32::from_rgb(140, 152, 176),
        Kind::Folder => Color32::from_rgb(240, 190, 70),
        Kind::Calc => Color32::from_rgb(167, 139, 250),
        Kind::Web => Color32::from_rgb(244, 114, 182),
        Kind::Clip => Color32::from_rgb(110, 205, 130),
        Kind::Snippet => Color32::from_rgb(250, 204, 21),
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Normal,
    Admin,
    Reveal,
    CopyPath,
    Terminal,
    CopyText,
}

#[derive(Clone, Copy, PartialEq)]
enum HotkeyTarget {
    Main,
    Clip,
}

/// (started at, window position at start, last polled position)
type DragTrack = (Instant, (i32, i32), (i32, i32));

pub struct App {
    shared: Arc<Shared>,
    engine: Engine,
    palette: Palette,

    query: String,
    results: Vec<Item>,
    selected: usize,
    user_moved: bool,
    scroll_to_selected: bool,
    dirty: bool,
    /// The launcher was opened over a file dialog (see `platform::dialog_jump`).
    dialog: bool,

    // Everything
    req_tx: Sender<SearchRequest>,
    rep_rx: Receiver<SearchReply>,
    ev_path: Arc<Mutex<String>>,
    req_id: u64,
    sent_query: String,
    files: Vec<FileHit>,
    files_query: String,
    ev_error: Option<EvError>,
    ev_ok: bool,

    // Apps
    apps_rx: Receiver<Vec<AppEntry>>,
    apps_tx: Sender<Vec<AppEntry>>,
    bm_rx: Receiver<Vec<crate::bookmarks::Bookmark>>,
    bm_tx: Sender<Vec<crate::bookmarks::Bookmark>>,
    last_bm_load: Instant,
    last_scan: Instant,
    scanning: bool,

    // window state
    focus_frames: u8,
    /// Set while the user drags the window: (started, position at start, last polled position).
    drag: Option<DragTrack>,
    /// A fresh mouse press happened and has not started a window drag yet.
    drag_armed: bool,
    was_focused: bool,
    last_height: f32,
    positioned: bool,
    background: bool,

    // settings panel
    panel_open: bool,
    ui_everything_path: String,
    ui_new_exclude: String,
    ui_autostart: bool,
    recording: Option<HotkeyTarget>,
    rec_win: bool,
    hotkey_warned: bool,

    // clipboard history
    clip_settings: Arc<ClipSettings>,
    /// Set by the watcher thread whenever a new clip was stored.
    clip_changed: Arc<AtomicBool>,
    /// Thumbnail textures for image clips currently listed (dropped when the window hides).
    thumbs: HashMap<i64, Option<egui::TextureHandle>>,
    ui_ignore_apps: String,

    // real file/app icons (loaded on a background thread)
    icons: crate::icons::IconLoader,
    icon_tex: HashMap<String, Option<egui::TextureHandle>>,
    icon_pending: std::collections::HashSet<String>,

    /// A dangerous command waiting for its second Enter.
    confirm: Option<(String, Instant)>,

    toast: Option<(String, Instant)>,
}

fn install_fonts(ctx: &egui::Context) {
    // Memory-map a system CJK font instead of reading ~20 MB into the heap: only the glyph pages
    // we actually draw become resident.
    let windir = std::env::var("WINDIR").unwrap_or_else(|_| r"C:\Windows".into());
    let candidates = [
        "msyh.ttc",
        "msyh.ttf",
        "Dengl.ttf",
        "simhei.ttf",
        "simsun.ttc",
    ];
    for name in candidates {
        let path = std::path::Path::new(&windir).join("Fonts").join(name);
        let Ok(file) = std::fs::File::open(&path) else {
            continue;
        };
        let Ok(map) = (unsafe { memmap2::Mmap::map(&file) }) else {
            continue;
        };
        // Leak the mapping so the bytes live for the whole process (egui wants 'static data).
        let map: &'static memmap2::Mmap = Box::leak(Box::new(map));
        let bytes: &'static [u8] = &map[..];
        let mut fonts = egui::FontDefinitions::default();
        fonts
            .font_data
            .insert("cjk".into(), Arc::new(egui::FontData::from_static(bytes)));
        for fam in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
            fonts
                .families
                .entry(fam)
                .or_default()
                .insert(0, "cjk".into());
        }
        ctx.set_fonts(fonts);
        return;
    }
}

fn apply_style(ctx: &egui::Context, p: &Palette, dark: bool) {
    let mut v = if dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    v.override_text_color = Some(p.text);
    v.selection.bg_fill = p.accent.gamma_multiply(0.45);
    v.selection.stroke = Stroke::new(1.0, p.accent);
    v.widgets.inactive.bg_fill = p.input_bg;
    v.widgets.hovered.bg_fill = p.hover;
    v.widgets.active.bg_fill = p.sel;
    v.window_shadow = egui::Shadow::NONE;
    v.popup_shadow = egui::Shadow::NONE;
    ctx.set_visuals(v);
    ctx.all_styles_mut(|s| {
        s.spacing.item_spacing = Vec2::new(8.0, 6.0);
        s.spacing.button_padding = Vec2::new(10.0, 5.0);
        s.interaction.selectable_labels = false;
    });
}

impl App {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        cfg: Config,
        shared: Arc<Shared>,
        background: bool,
    ) -> Self {
        let ctx = cc.egui_ctx.clone();
        install_fonts(&ctx);
        let palette = if cfg.dark_mode {
            Palette::dark()
        } else {
            Palette::light()
        };
        apply_style(&ctx, &palette, cfg.dark_mode);

        let learner = Learner::load(config::data_dir().join("learn.json"));
        let mut engine = Engine::new(cfg.clone(), learner);
        let cached = apps::load_cache();
        engine.rebuild(&cached);

        // Clipboard history: one shared handle for the UI engine and the watcher thread.
        let clip_settings = ClipSettings::from_config(&cfg);
        let clip_changed = Arc::new(AtomicBool::new(false));
        let mut clip_error: Option<String> = None;
        match clipdb::Store::open(&clipboard::db_path()) {
            Ok(store) => {
                engine.set_clip_store(store.clone());
                let wake_ctx = ctx.clone();
                clipboard::spawn_listener(
                    store,
                    clip_settings.clone(),
                    clip_changed.clone(),
                    Box::new(move || wake_ctx.request_repaint()),
                );
            }
            Err(e) => clip_error = Some(format!("剪贴板数据库无法打开: {e}")),
        }

        // Everything worker.
        let ev_path = Arc::new(Mutex::new(cfg.everything_path.clone()));
        let (req_tx, req_rx) = channel();
        let (rep_tx, rep_rx) = channel();
        let wake_ctx = ctx.clone();
        everything::spawn_worker(
            ev_path.clone(),
            req_rx,
            rep_tx,
            Box::new(move || wake_ctx.request_repaint()),
        );

        // App scanner (background).
        let (apps_tx, apps_rx) = channel();
        let (bm_tx, bm_rx) = channel();
        let mut app = Self {
            shared: shared.clone(),
            engine,
            palette,
            query: String::new(),
            results: Vec::new(),
            selected: 0,
            user_moved: false,
            scroll_to_selected: false,
            dirty: true,
            dialog: false,
            req_tx,
            rep_rx,
            ev_path,
            req_id: 0,
            sent_query: String::new(),
            files: Vec::new(),
            files_query: String::new(),
            ev_error: None,
            ev_ok: false,
            apps_rx,
            apps_tx,
            bm_rx,
            bm_tx,
            last_bm_load: Instant::now(),
            last_scan: Instant::now(),
            scanning: false,
            focus_frames: 3,
            drag: None,
            drag_armed: false,
            was_focused: false,
            last_height: 0.0,
            positioned: false,
            background,
            panel_open: false,
            ui_everything_path: cfg.everything_path.clone(),
            ui_new_exclude: String::new(),
            ui_autostart: platform::autostart_enabled(),
            recording: None,
            rec_win: false,
            hotkey_warned: false,
            clip_settings,
            clip_changed,
            thumbs: HashMap::new(),
            ui_ignore_apps: cfg.clipboard_ignore_apps.join(", "),
            icons: {
                let c = ctx.clone();
                crate::icons::IconLoader::spawn_system(Box::new(move || c.request_repaint()))
            },
            icon_tex: HashMap::new(),
            icon_pending: std::collections::HashSet::new(),
            confirm: None,
            toast: None,
        };
        app.start_scan(&ctx);
        app.load_bookmarks(&ctx);
        if let Some(e) = clip_error {
            app.toast(e);
        }

        // Hotkey + tray thread.
        let wake_ctx = ctx.clone();
        platform::spawn_hotkey_and_tray(shared, Arc::new(move || wake_ctx.request_repaint()));
        app
    }

    fn start_scan(&mut self, ctx: &egui::Context) {
        if self.scanning {
            return;
        }
        self.scanning = true;
        self.last_scan = Instant::now();
        let tx = self.apps_tx.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let apps = apps::rescan();
            let _ = tx.send(apps);
            ctx.request_repaint();
        });
    }

    /// Re-read browser bookmarks on a helper thread (cheap, but it touches the disk).
    fn load_bookmarks(&mut self, ctx: &egui::Context) {
        if !self.engine.cfg.show_bookmarks {
            return;
        }
        self.last_bm_load = Instant::now();
        let tx = self.bm_tx.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(crate::bookmarks::load_all());
            ctx.request_repaint();
        });
    }

    fn toast(&mut self, msg: impl Into<String>) {
        self.toast = Some((msg.into(), Instant::now()));
    }

    // ───────────────────────────── state updates ─────────────────────────────

    fn on_shown(&mut self, ctx: &egui::Context) {
        if self.last_bm_load.elapsed() > Duration::from_secs(120) {
            self.load_bookmarks(ctx);
        }
        if self.recording.is_some() {
            self.stop_recording();
        }
        self.confirm = None;
        self.dialog = self.shared.dialog_hwnd.load(Ordering::Relaxed) != 0;
        self.thumbs.clear();
        self.query.clear();
        if let Some(q) = self
            .shared
            .pending_query
            .lock()
            .ok()
            .and_then(|mut g| g.take())
        {
            self.query = q;
        }
        self.files.clear();
        self.files_query.clear();
        self.sent_query.clear();
        self.panel_open = self.shared.open_panel.swap(false, Ordering::SeqCst);
        self.focus_frames = 4;
        self.was_focused = false;
        self.selected = 0;
        self.user_moved = false;
        self.dirty = true;
        if self.last_scan.elapsed() > RESCAN_EVERY {
            self.start_scan(ctx);
        }
    }

    fn poll_channels(&mut self) {
        while let Ok(list) = self.bm_rx.try_recv() {
            self.engine.set_bookmarks(list);
            self.dirty = true;
        }
        while let Ok(apps) = self.apps_rx.try_recv() {
            self.scanning = false;
            self.engine.rebuild(&apps);
            self.dirty = true;
        }
        while let Ok(rep) = self.rep_rx.try_recv() {
            if rep.id != self.req_id {
                continue; // stale
            }
            match rep.result {
                Ok(hits) => {
                    self.files = hits;
                    self.files_query = self.sent_query.clone();
                    self.ev_error = None;
                    self.ev_ok = true;
                }
                Err(e) => {
                    self.files.clear();
                    self.ev_error = Some(e);
                    self.ev_ok = false;
                }
            }
            self.dirty = true;
        }
    }

    fn send_search_if_needed(&mut self) {
        let q = self.query.trim().to_string();
        if q == self.sent_query {
            return;
        }
        self.sent_query = q.clone();
        self.user_moved = false;
        self.selected = 0;
        self.dirty = true;
        if q.is_empty() || !self.engine.cfg.show_files || self.is_clip_query(&q) || crate::pathnav::is_path_query(&q) || crate::recent::trigger(&q).is_some() || q.trim_start().starts_with('>') {
            self.files.clear();
            self.files_query.clear();
            return;
        }
        self.req_id += 1;
        // "f report": the scope letter is ours, Everything only sees "report" (and nothing at all
        // for app/settings scopes).
        let mut search_q = q.as_str();
        if let Some((scope, rest)) = self.engine.split_scope(&q) {
            if scope != crate::engine::Scope::Files || rest.is_empty() {
                self.files.clear();
                self.files_query.clear();
                return;
            }
            search_q = rest;
        }
        let text = everything::search_text(search_q, &self.engine.cfg.excluded_dirs);
        let _ = self.req_tx.send(SearchRequest {
            id: self.req_id,
            text,
            max: self.engine.cfg.max_file_results,
        });
    }

    fn is_clip_query(&self, q: &str) -> bool {
        if !self.engine.cfg.clipboard_enabled {
            return false;
        }
        let first = q.split_whitespace().next().unwrap_or("");
        first.eq_ignore_ascii_case("cb") || first.eq_ignore_ascii_case("clip")
    }

    fn recompute(&mut self) {
        let keep_id = if self.user_moved {
            self.results.get(self.selected).map(|i| i.id.clone())
        } else {
            None
        };
        let fresh = self.files_query == self.query.trim();
        self.results = if self.dialog && self.query.trim().is_empty() {
            self.engine.dialog_folders()
        } else {
            self.engine.results(&self.query, &self.files, fresh)
        };
        self.selected = keep_id
            .and_then(|id| self.results.iter().position(|i| i.id == id))
            .unwrap_or(0)
            .min(self.results.len().saturating_sub(1));
        self.dirty = false;
    }

    // ───────────────────────────── actions ─────────────────────────────

    /// The OS runs its own move loop during a drag, so we cannot see the mouse release. Instead
    /// we note the start and poll the window position until it stops changing.
    fn begin_drag(&mut self) {
        if let Some(p) = platform::window_pos(&self.shared) {
            self.drag = Some((Instant::now(), p, p));
        }
    }

    /// Dragging works from *anywhere* in the window, except where the drag already means
    /// something else (selecting text, moving a slider or scrollbar). An empty search box has no
    /// text to select, so it drags the window too.
    fn window_drag(&mut self, ctx: &egui::Context) {
        let (pressed, down, dragging) =
            ctx.input(|i| (i.pointer.primary_pressed(), i.pointer.primary_down(), i.pointer.is_decidedly_dragging()));
        if pressed {
            self.drag_armed = true;
        }
        if !down {
            self.drag_armed = false;
        }
        if !(self.drag_armed && dragging) {
            return;
        }
        let taken = match ctx.dragged_id() {
            None => false,
            Some(id) => !(id == egui::Id::new("query") && self.query.is_empty()),
        };
        if !taken {
            self.drag_armed = false; // the OS move loop swallows the release; re-arm on next press
            ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
            self.begin_drag();
        }
    }

    fn track_drag(&mut self, ctx: &egui::Context) {
        let Some((started, start, prev)) = self.drag else { return };
        let Some(cur) = platform::window_pos(&self.shared) else {
            self.drag = None;
            return;
        };
        if cur != start && cur == prev {
            // Moved and now still: remember the new spot.
            if let Ok(mut g) = self.shared.saved_pos.lock() {
                *g = Some(cur);
            }
            self.engine.cfg.window_pos = Some(cur);
            self.engine.cfg.save();
            self.drag = None;
            self.focus_frames = 3; // the drag took keyboard focus from the input box
        } else if started.elapsed() > Duration::from_secs(20) {
            self.drag = None;
        } else {
            self.drag = Some((started, start, cur));
            ctx.request_repaint_after(Duration::from_millis(150));
        }
    }

    fn hide(&mut self) {
        if self.recording.is_some() {
            self.stop_recording();
        }
        self.thumbs.clear();
        platform::hide_window(&self.shared);
    }

    /// Content of a clipboard-history item, if `item` is one.
    fn clip_content(&self, item: &Item) -> Option<ClipContent> {
        match &item.action {
            Action::PasteClip { id, image } => self.engine.clip.content(*id, *image),
            _ => None,
        }
    }

    /// Copy a history item to the clipboard without pasting.
    fn copy_clip(&mut self, ctx: &egui::Context, c: ClipContent) {
        let ok = platform::set_clip(&c);
        match (ok, &c) {
            (true, _) => self.toast("已复制到剪贴板"),
            (false, ClipContent::Text(t)) => {
                ctx.copy_text(t.clone());
                self.toast("已复制到剪贴板");
            }
            (false, ClipContent::Png(_)) => self.toast("复制图片失败"),
        }
    }

    fn activate(&mut self, ctx: &egui::Context, idx: usize, mode: Mode) {
        let Some(item) = self.results.get(idx).cloned() else {
            return;
        };
        let query = self.query.clone();
        // Items listed above the chosen one are "skipped" for this query (nudged down next time).
        let skipped: Vec<String> = self
            .results
            .iter()
            .take(idx.min(3))
            .filter(|i| !matches!(i.kind, Kind::Calc | Kind::Web | Kind::Clip))
            .map(|i| i.id.clone())
            .collect();

        // Dangerous commands need a second Enter.
        if matches!(mode, Mode::Normal | Mode::Admin) {
            let needs_confirm = item
                .id
                .strip_prefix("cmd:")
                .and_then(|n| {
                    self.engine
                        .cfg
                        .commands
                        .iter()
                        .find(|c| c.name.eq_ignore_ascii_case(n))
                })
                .map(|c| c.confirm)
                .unwrap_or(false);
            if needs_confirm {
                let confirmed = matches!(&self.confirm, Some((id, t)) if *id == item.id && t.elapsed() < Duration::from_secs(5));
                if !confirmed {
                    self.confirm = Some((item.id.clone(), Instant::now()));
                    self.toast(format!(
                        "再按一次 Enter 确认执行「{}」 (Esc 取消)",
                        item.title
                    ));
                    return;
                }
                self.confirm = None;
            }
        }

        match mode {
            Mode::CopyPath => {
                if let Some(p) = &item.path {
                    ctx.copy_text(p.clone());
                    self.toast("已复制路径");
                }
                return;
            }
            Mode::CopyText => {
                if let Some(c) = self.clip_content(&item) {
                    self.copy_clip(ctx, c);
                }
                return;
            }
            Mode::Reveal => {
                if let Some(c) = self.clip_content(&item) {
                    self.copy_clip(ctx, c);
                } else if let Some(p) = &item.path {
                    platform::reveal_in_explorer(p);
                    self.engine.learn(&item, &query, &skipped);
                    self.hide();
                }
                return;
            }
            Mode::Terminal => {
                let Some(p) = &item.path else {
                    self.toast("该项没有对应的文件夹");
                    return;
                };
                let dir = if item.kind == Kind::Folder {
                    p.clone()
                } else {
                    p.rsplit_once(['\\', '/'])
                        .map(|(d, _)| d.to_string())
                        .unwrap_or_default()
                };
                let dir = dir.trim_end_matches(['\\', '/']).to_string();
                if dir.is_empty() {
                    self.toast("无法确定所在文件夹");
                    return;
                }
                // Prefer Windows Terminal; a bare drive root ("C:") cannot be quoted safely.
                let opened = if dir.ends_with(':') {
                    platform::shell_open("cmd.exe", "", false, Some(&format!("{dir}\\")))
                } else {
                    platform::shell_open("wt.exe", &format!("-d \"{dir}\""), false, None)
                        .or_else(|_| platform::shell_open("cmd.exe", "", false, Some(&dir)))
                };
                match opened {
                    Ok(()) => self.hide(),
                    Err(e) => self.toast(e),
                }
                return;
            }
            _ => {}
        }
        // Inside an Open/Save dialog, files and folders navigate the dialog instead of opening.
        if self.dialog && mode == Mode::Normal && matches!(item.kind, Kind::File | Kind::Folder) {
            if let Some(p) = &item.path {
                let (folder, file) = if item.kind == Kind::Folder {
                    (p.trim_end_matches(['\\', '/']).to_string(), None)
                } else {
                    match p.rsplit_once(['\\', '/']) {
                        Some((d, f)) => (d.to_string(), Some(f.to_string())),
                        None => (p.clone(), None),
                    }
                };
                let folder = if folder.ends_with(':') { format!("{folder}\\") } else { folder };
                match platform::dialog_jump(&self.shared, &folder, file.as_deref()) {
                    Ok(()) => {
                        self.engine.learn(&item, &query, &skipped);
                        self.hide();
                        return;
                    }
                    Err(e) => {
                        self.dialog = false;
                        self.toast(format!("{e}，改为直接打开"));
                    }
                }
            }
        }
        match &item.action {
            Action::Open {
                target,
                args,
                admin,
                cwd,
            } => {
                let admin = *admin || (mode == Mode::Admin && item.can_run_admin());
                match platform::shell_open(target, args, admin, cwd.as_deref()) {
                    Ok(()) => {
                        self.engine.learn(&item, &query, &skipped);
                        self.hide();
                    }
                    Err(e) => self.toast(e),
                }
            }
            Action::Copy(text) => {
                ctx.copy_text(text.clone());
                self.engine.learn(&item, &query, &skipped);
                self.toast("结果已复制");
            }
            Action::PasteClip { .. } => {
                // Paste into the window that had focus before the launcher opened.
                match self.clip_content(&item) {
                    Some(c) => {
                        if platform::paste_clip(&c) {
                            self.hide();
                        } else {
                            self.copy_clip(ctx, c);
                        }
                    }
                    None => self.toast("该记录已不存在"),
                }
            }
            Action::PasteText(text) => {
                let c = ClipContent::Text(crate::snippets::expand(text, platform::local_now()));
                self.engine.learn(&item, &query, &skipped);
                if platform::paste_clip(&c) {
                    self.hide();
                } else {
                    self.copy_clip(ctx, c);
                }
            }
            Action::Quit => std::process::exit(0),
            Action::OpenPanel => {
                self.panel_open = true;
                self.engine.learn(&item, &query, &skipped);
            }
        }
    }

    // ───────────────────────────── hotkeys ─────────────────────────────

    fn start_recording(&mut self, target: HotkeyTarget) {
        self.recording = Some(target);
        self.rec_win = false;
        // Release the global hotkeys so pressing the current combination reaches the recorder.
        self.shared.suspended.store(true, Ordering::SeqCst);
        platform::reapply_hotkey(&self.shared);
    }

    fn stop_recording(&mut self) {
        self.recording = None;
        self.shared.suspended.store(false, Ordering::SeqCst);
        platform::reapply_hotkey(&self.shared);
    }

    fn set_hotkey(&mut self, target: HotkeyTarget, value: String) {
        match target {
            HotkeyTarget::Main => {
                self.engine.cfg.hotkey = value.clone();
                if let Ok(mut h) = self.shared.hotkey.lock() {
                    *h = value;
                }
            }
            HotkeyTarget::Clip => {
                self.engine.cfg.clipboard_hotkey = value.clone();
                if let Ok(mut h) = self.shared.clip_hotkey.lock() {
                    *h = value;
                }
            }
        }
        self.engine.cfg.save();
        self.recording = None;
        self.shared.suspended.store(false, Ordering::SeqCst);
        platform::reapply_hotkey(&self.shared);
    }

    /// While recording, swallow every key and turn the first valid combination into a hotkey.
    fn capture_hotkey(&mut self, ctx: &egui::Context, target: HotkeyTarget) {
        let events: Vec<egui::Event> = ctx.input(|i| i.events.clone());
        let mut done: Option<String> = None;
        let mut cancel = false;
        let mut hint = false;
        for ev in &events {
            if let egui::Event::Key {
                key,
                pressed: true,
                repeat: false,
                modifiers,
                ..
            } = ev
            {
                if *key == Key::Escape && !(modifiers.ctrl || modifiers.alt || modifiers.shift) {
                    cancel = true;
                    break;
                }
                match platform::compose_hotkey(
                    modifiers.ctrl,
                    modifiers.alt,
                    modifiers.shift,
                    self.rec_win,
                    key.name(),
                ) {
                    Some(h) => {
                        done = Some(h);
                        break;
                    }
                    None => hint = true,
                }
            }
        }
        ctx.input_mut(|i| {
            i.events
                .retain(|e| !matches!(e, egui::Event::Key { .. } | egui::Event::Text(_)))
        });
        if cancel {
            self.stop_recording();
        } else if let Some(h) = done {
            self.set_hotkey(target, h.clone());
            self.toast(format!("快捷键已设为 {h}"));
        } else if hint {
            self.toast("需要至少一个修饰键 (Ctrl / Alt / Shift / 勾选 Win) 加一个主键");
        }
    }

    fn handle_keys(&mut self, ctx: &egui::Context) {
        if let Some(target) = self.recording {
            self.capture_hotkey(ctx, target);
            return;
        }
        let n = self.results.len();
        let mut act: Option<(usize, Mode)> = None;
        let mut hide = false;
        let mut toggle_panel = false;
        let mut exclude_sel = false;
        let mut forget_sel = false;
        let mut bulk_del = false;
        let mut copy_sel = false;
        let mut pin_sel = false;
        let mut moved: Option<i32> = None;
        let mut nav_complete = false;
        let mut nav_up = false;
        let nav_mode = crate::pathnav::is_path_query(&self.query);
        let caret_at_end = self.caret_at_end(ctx);

        ctx.input_mut(|i| {
            if i.consume_key(Modifiers::CTRL, Key::Comma) {
                toggle_panel = true;
            }
            if self.panel_open {
                if i.consume_key(Modifiers::NONE, Key::Escape) {
                    self.panel_open = false;
                }
                return;
            }
            if i.consume_key(Modifiers::NONE, Key::ArrowDown)
                || i.consume_key(Modifiers::CTRL, Key::J)
            {
                moved = Some(1);
            }
            if i.consume_key(Modifiers::NONE, Key::ArrowUp)
                || i.consume_key(Modifiers::CTRL, Key::K)
            {
                moved = Some(-1);
            }
            // Path browsing: Tab (or →, with the caret at the end) accepts the selected entry;
            // Alt+← goes up a level. Tab is always consumed so it can never steal focus.
            if i.consume_key(Modifiers::NONE, Key::Tab) {
                nav_complete = nav_mode;
            }
            if nav_mode && caret_at_end && i.consume_key(Modifiers::NONE, Key::ArrowRight) {
                nav_complete = true;
            }
            if nav_mode && i.consume_key(Modifiers::ALT, Key::ArrowLeft) {
                nav_up = true;
            }
            if i.consume_key(Modifiers::NONE, Key::PageDown) {
                moved = Some(8);
            }
            if i.consume_key(Modifiers::NONE, Key::PageUp) {
                moved = Some(-8);
            }
            if i.consume_key(Modifiers::CTRL, Key::Enter) {
                act = Some((self.selected, Mode::Admin));
            } else if i.consume_key(Modifiers::SHIFT, Key::Enter) {
                act = Some((self.selected, Mode::Reveal));
            } else if i.consume_key(Modifiers::NONE, Key::Enter) {
                act = Some((self.selected, Mode::Normal));
            }
            for (k, idx) in [
                (Key::Num1, 0),
                (Key::Num2, 1),
                (Key::Num3, 2),
                (Key::Num4, 3),
                (Key::Num5, 4),
                (Key::Num6, 5),
                (Key::Num7, 6),
                (Key::Num8, 7),
                (Key::Num9, 8),
            ] {
                if i.consume_key(Modifiers::CTRL, k) {
                    act = Some((idx, Mode::Normal));
                }
            }
            if i.consume_key(Modifiers::CTRL | Modifiers::SHIFT, Key::C) {
                copy_sel = true;
            }
            if i.consume_key(Modifiers::CTRL, Key::P) {
                pin_sel = true;
            }
            if i.consume_key(Modifiers::CTRL, Key::T) {
                act = Some((self.selected, Mode::Terminal));
            }
            if i.consume_key(Modifiers::CTRL, Key::D) {
                exclude_sel = true;
            }
            if i.consume_key(Modifiers::ALT, Key::Delete) {
                bulk_del = true;
            } else if i.consume_key(Modifiers::CTRL, Key::Delete) {
                forget_sel = true;
            }
            if i.consume_key(Modifiers::NONE, Key::Escape) {
                hide = true;
            }
        });

        if toggle_panel {
            self.panel_open = !self.panel_open;
        }
        if self.panel_open {
            return;
        }
        if let Some(d) = moved {
            if n > 0 {
                let next = (self.selected as i32 + d).clamp(0, n as i32 - 1) as usize;
                self.selected = next;
                self.user_moved = true;
                self.scroll_to_selected = true;
            }
        }
        if nav_complete {
            if let Some(it) = self.results.get(self.selected) {
                if it.subtitle != crate::engine::OPEN_DIR_SUBTITLE && it.id != "nav:slow" {
                    let text = crate::pathnav::complete(&self.query, &it.title, it.kind == Kind::Folder);
                    self.set_query(ctx, text);
                }
            }
        }
        if nav_up {
            if let Some(text) = crate::pathnav::parent_query(&self.query) {
                self.set_query(ctx, text);
            }
        }
        if let Some((idx, mode)) = act {
            self.activate(ctx, idx, mode);
        }
        if copy_sel {
            self.activate(ctx, self.selected, Mode::CopyPath);
        }
        if exclude_sel {
            self.exclude_selected();
        }
        if pin_sel {
            self.pin_selected();
        }
        if forget_sel {
            if let Some(it) = self.results.get(self.selected).cloned() {
                self.engine.forget(&it);
                if it.kind == Kind::Clip {
                    self.thumbs.clear();
                    self.toast("已删除该剪贴板记录");
                } else {
                    self.toast("已清除该项的学习记录");
                }
                self.dirty = true;
            }
        }
        if bulk_del {
            self.bulk_delete_clips();
        }
        if hide {
            self.confirm = None;
            if !self.query.is_empty() {
                self.query.clear();
            } else {
                self.hide();
            }
        }
    }

    /// Alt+Delete in a clipboard query: delete every (unpinned) record matching the
    /// filter, after a second press to confirm.
    fn bulk_delete_clips(&mut self) {
        let Some(filter) = crate::engine::Engine::clip_filter(&self.query).map(str::to_string) else {
            self.toast("批量删除：先输入 cb 关键词，例如 cb magnet");
            return;
        };
        if filter.is_empty() {
            self.toast("批量删除需要关键词，例如 cb magnet (清空全部请到设置)");
            return;
        }
        let id = format!("clipbulk:{filter}");
        let n = self.engine.clip.matching_unpinned(&filter).len();
        if n == 0 {
            self.toast("没有可删除的匹配记录 (置顶项不会被删除)");
            return;
        }
        let confirmed = matches!(&self.confirm, Some((c, t)) if *c == id && t.elapsed() < Duration::from_secs(5));
        if !confirmed {
            self.confirm = Some((id, Instant::now()));
            self.toast(format!("将删除 {n} 条含「{filter}」的记录 — 再按一次 Alt+Delete 确认 (置顶项保留)"));
            return;
        }
        self.confirm = None;
        let done = self.engine.clip.delete_matching(&filter);
        self.thumbs.clear();
        self.dirty = true;
        self.toast(format!("已删除 {done} 条记录"));
    }

    /// Replace the search text and put the caret at its end.
    fn set_query(&mut self, ctx: &egui::Context, text: String) {
        let id = egui::Id::new("query");
        let end = text.chars().count();
        self.query = text;
        let mut state = egui::text_edit::TextEditState::load(ctx, id).unwrap_or_default();
        state.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(end))));
        state.store(ctx, id);
        self.focus_frames = self.focus_frames.max(1);
    }

    fn caret_at_end(&self, ctx: &egui::Context) -> bool {
        let end = self.query.chars().count();
        egui::text_edit::TextEditState::load(ctx, egui::Id::new("query"))
            .and_then(|s| s.cursor.char_range())
            .is_none_or(|r| r.primary.index.0 >= end && r.secondary.index.0 >= end)
    }

    fn pin_selected(&mut self) {
        let Some(it) = self.results.get(self.selected).cloned() else {
            return;
        };
        match self.engine.toggle_pin(&it) {
            Some(true) => self.toast("已置顶 (不会被自动清理)"),
            Some(false) => self.toast("已取消置顶"),
            None => self.toast("只有剪贴板记录可以置顶"),
        }
        self.dirty = true;
    }

    fn exclude_selected(&mut self) {
        let Some(it) = self.results.get(self.selected).cloned() else {
            return;
        };
        match it.path {
            Some(p) if matches!(it.kind, Kind::File | Kind::Folder) => {
                self.engine.exclude(&p);
                self.toast(format!("已忽略: {p}"));
                self.dirty = true;
            }
            _ => self.toast("只有文件/文件夹可以被忽略"),
        }
    }

    // ───────────────────────────── drawing ─────────────────────────────

    fn wanted_height(&self) -> f32 {
        if self.panel_open {
            return PANEL_H;
        }
        let rows = self.results.len().min(self.engine.cfg.visible_rows.max(3));
        if rows == 0 && self.query.trim().is_empty() {
            return HEADER_H + PAD;
        }
        HEADER_H + (rows.max(1) as f32) * ROW_H + FOOTER_H + PAD * 1.5 + 14.0 // + widget spacing; was clipping the status line
    }

    fn draw_header(&mut self, ui: &mut egui::Ui) {
        let p = self.palette;
        let (rect, _) = ui.allocate_exact_size(
            Vec2::new(ui.available_width(), HEADER_H - PAD),
            Sense::hover(),
        );
        let input_rect = rect.shrink2(Vec2::new(0.0, 4.0));
        ui.painter()
            .rect_filled(input_rect, CornerRadius::same(12), p.input_bg);

        // Magnifier glyph doubles as the drag handle for the borderless window.
        let icon_rect = Rect::from_min_size(input_rect.min, Vec2::new(48.0, input_rect.height()));
        let c = icon_rect.center() + Vec2::new(-2.0, -2.0);
        ui.painter()
            .circle_stroke(c, 8.0, Stroke::new(2.2, p.accent));
        ui.painter().line_segment(
            [c + Vec2::new(6.0, 6.0), c + Vec2::new(12.0, 12.0)],
            Stroke::new(2.6, p.accent),
        );

        let edit_rect = Rect::from_min_max(
            Pos2::new(input_rect.left() + 48.0, input_rect.top()),
            Pos2::new(input_rect.right() - 14.0, input_rect.bottom()),
        );
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(edit_rect)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        let resp = child.add(
            egui::TextEdit::singleline(&mut self.query)
                .id(egui::Id::new("query"))
                .frame(egui::Frame::NONE)
                .hint_text("搜索文件、应用、Windows 设置、命令…")
                .font(FontId::proportional(22.0))
                .desired_width(edit_rect.width())
                .margin(Vec2::new(0.0, 0.0)),
        );
        if resp.changed() {
            ui.ctx().request_repaint();
        }
        if self.focus_frames > 0 {
            resp.request_focus();
            self.focus_frames -= 1;
        }
    }

    /// Turn finished icon loads into textures.
    fn poll_icons(&mut self, ctx: &egui::Context) {
        if self.icon_tex.len() > 400 {
            self.icon_tex.clear(); // plenty for any result list; cheap to refill
        }
        while let Ok((key, img)) = self.icons.rx.try_recv() {
            self.icon_pending.remove(&key);
            let tex = img.map(|i| {
                let image = egui::ColorImage::from_rgba_unmultiplied([i.w, i.h], &i.rgba);
                ctx.load_texture(format!("icon:{key}"), image, egui::TextureOptions::LINEAR)
            });
            self.icon_tex.insert(key, tex);
        }
    }

    /// Ask for the icon of a visible row if it is not cached or already requested.
    fn request_icon(&mut self, idx: usize) {
        let Some(spec) = self.results.get(idx).and_then(crate::icons::spec_for) else { return };
        if self.icon_tex.contains_key(&spec.key) || self.icon_pending.contains(&spec.key) {
            return;
        }
        self.icon_pending.insert(spec.key.clone());
        self.icons.request(spec);
    }

    fn ensure_thumb(&mut self, ctx: &egui::Context, idx: usize) {
        let Some((id, true)) = self.results.get(idx).and_then(Engine::clip_id) else {
            return;
        };
        if self.thumbs.contains_key(&id) {
            return;
        }
        let tex = self
            .engine
            .clip
            .get_thumb(id)
            .and_then(|b| clipimg::decode_png(&b))
            .map(|img| {
                let image = egui::ColorImage::from_rgba_unmultiplied(
                    [img.w as usize, img.h as usize],
                    &img.px,
                );
                ctx.load_texture(format!("clip{id}"), image, egui::TextureOptions::LINEAR)
            });
        self.thumbs.insert(id, tex);
    }

    fn draw_row(&mut self, ui: &mut egui::Ui, idx: usize, rect: Rect) -> egui::Response {
        let p = self.palette;
        let item = &self.results[idx];
        let resp = ui.interact(rect, egui::Id::new(("row", idx)), Sense::click());
        if resp.hovered() && ui.input(|i| i.pointer.delta() != Vec2::ZERO) {
            self.selected = idx;
            self.user_moved = true;
        }
        let selected = idx == self.selected;
        let painter = ui.painter();
        if selected {
            painter.rect_filled(rect, CornerRadius::same(10), p.sel);
            painter.rect_stroke(
                rect,
                CornerRadius::same(10),
                Stroke::new(1.0, p.accent.gamma_multiply(0.5)),
                StrokeKind::Inside,
            );
        } else if resp.hovered() {
            painter.rect_filled(rect, CornerRadius::same(10), p.hover);
        }

        // Kind badge.
        let badge = Rect::from_center_size(
            Pos2::new(rect.left() + 28.0, rect.center().y),
            Vec2::splat(30.0),
        );
        let kc = kind_color(item.kind);
        let icon = crate::icons::spec_for(item)
            .and_then(|s| self.icon_tex.get(&s.key))
            .and_then(|t| t.as_ref())
            .map(|t| t.id());
        if let Some(tex_id) = icon {
            // The real icon, a little larger than the badge it replaces.
            let r = Rect::from_center_size(badge.center(), Vec2::splat(32.0));
            painter.image(tex_id, r, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
        } else {
            painter.rect_filled(badge, CornerRadius::same(8), kc.gamma_multiply(0.22));
            let glyph = item.kind.label().chars().next().unwrap_or('?');
            painter.text(
                badge.center(),
                Align2::CENTER_CENTER,
                glyph,
                FontId::proportional(15.0),
                kc,
            );
        }
        // Image clips: show the thumbnail (fitted into a 38px box) over the badge.
        if let Some((tex_id, size)) = Engine::clip_id(item)
            .filter(|(_, img)| *img)
            .and_then(|(id, _)| self.thumbs.get(&id))
            .and_then(|t| t.as_ref())
            .map(|t| (t.id(), t.size_vec2()))
        {
            let scale = (38.0 / size.x).min(38.0 / size.y);
            let r = Rect::from_center_size(badge.center(), size * scale);
            painter.rect_filled(r.expand(1.0), CornerRadius::same(4), p.border);
            painter.image(
                tex_id,
                r,
                Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                Color32::WHITE,
            );
        }

        // Title with matched text highlighted.
        let text_left = rect.left() + 54.0;
        let right_pad = 96.0;
        let clip = Rect::from_min_max(
            Pos2::new(text_left, rect.top()),
            Pos2::new(rect.right() - right_pad, rect.bottom()),
        );
        let p2 = painter.with_clip_rect(clip);
        let job = highlight_job(
            &item.title,
            &self.query,
            FontId::proportional(16.0),
            p.text,
            p.accent,
        );
        let galley = p2.layout_job(job);
        p2.galley(Pos2::new(text_left, rect.top() + 8.0), galley, p.text);
        p2.text(
            Pos2::new(text_left, rect.top() + 30.0),
            Align2::LEFT_TOP,
            ellipsize_middle(&item.subtitle, 84),
            FontId::proportional(12.0),
            p.dim,
        );

        // Right side: kind + quick-launch hint.
        painter.text(
            Pos2::new(rect.right() - 14.0, rect.top() + 10.0),
            Align2::RIGHT_TOP,
            item.kind.label(),
            FontId::proportional(11.5),
            kc,
        );
        if idx < 9 {
            painter.text(
                Pos2::new(rect.right() - 14.0, rect.top() + 28.0),
                Align2::RIGHT_TOP,
                format!("Ctrl+{}", idx + 1),
                FontId::proportional(11.0),
                p.dim.gamma_multiply(0.8),
            );
        }
        resp
    }

    fn draw_list(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let p = self.palette;
        let rows_visible = self.engine.cfg.visible_rows.max(3);
        let max_h = rows_visible as f32 * ROW_H;
        if self.results.is_empty() {
            let msg = if self.query.trim().is_empty() {
                ""
            } else if self.is_clip_query(&self.query) {
                "剪贴板中没有匹配的记录"
            } else {
                "没有匹配的结果"
            };
            ui.add_space(8.0);
            ui.vertical_centered(|ui| ui.label(egui::RichText::new(msg).color(p.dim)));
            return;
        }
        let mut clicked: Option<usize> = None;
        let mut ctx_action: Option<(usize, Mode)> = None;
        let mut ctx_exclude = false;
        let mut ctx_forget = false;
        let mut ctx_pin = false;
        egui::ScrollArea::vertical()
            .max_height(max_h)
            .auto_shrink([false, true])
            .show_rows(ui, ROW_H, self.results.len(), |ui, range| {
                for idx in range {
                    let (rect, _) = ui.allocate_exact_size(
                        Vec2::new(ui.available_width(), ROW_H - 2.0),
                        Sense::hover(),
                    );
                    self.ensure_thumb(ctx, idx);
                    self.request_icon(idx);
                    let resp = self.draw_row(ui, idx, rect);
                    if self.scroll_to_selected && idx == self.selected {
                        ui.scroll_to_rect(rect, None);
                        self.scroll_to_selected = false;
                    }
                    if resp.clicked() {
                        clicked = Some(idx);
                    }
                    let item = self.results[idx].clone();
                    resp.context_menu(|ui| {
                        self.selected = idx;
                        if item.kind == Kind::Clip
                            && ui.button("置顶 / 取消置顶  (Ctrl+P)").clicked()
                        {
                            self.selected = idx;
                            ctx_pin = true;
                            ui.close();
                        }
                        if item.kind == Kind::Clip && ui.button("仅复制  (Shift+Enter)").clicked()
                        {
                            ctx_action = Some((idx, Mode::CopyText));
                            ui.close();
                        }
                        if ui
                            .button(if item.kind == Kind::Clip {
                                "粘贴"
                            } else {
                                "打开"
                            })
                            .clicked()
                        {
                            ctx_action = Some((idx, Mode::Normal));
                            ui.close();
                        }
                        if item.can_run_admin()
                            && ui.button("以管理员身份运行  (Ctrl+Enter)").clicked()
                        {
                            ctx_action = Some((idx, Mode::Admin));
                            ui.close();
                        }
                        if item.path.is_some() {
                            if ui.button("打开所在文件夹  (Shift+Enter)").clicked() {
                                ctx_action = Some((idx, Mode::Reveal));
                                ui.close();
                            }
                            if ui.button("在此处打开终端  (Ctrl+T)").clicked() {
                                ctx_action = Some((idx, Mode::Terminal));
                                ui.close();
                            }
                            if ui.button("复制路径  (Ctrl+Shift+C)").clicked() {
                                ctx_action = Some((idx, Mode::CopyPath));
                                ui.close();
                            }
                        }
                        ui.separator();
                        if matches!(item.kind, Kind::File | Kind::Folder)
                            && ui.button("不再显示此项  (Ctrl+D)").clicked()
                        {
                            ctx_exclude = true;
                            ui.close();
                        }
                        if ui.button("忘记学习记录  (Ctrl+Delete)").clicked() {
                            ctx_forget = true;
                            ui.close();
                        }
                    });
                }
            });
        if let Some(i) = clicked {
            self.activate(ctx, i, Mode::Normal);
        }
        if let Some((i, m)) = ctx_action {
            self.activate(ctx, i, m);
        }
        if ctx_exclude {
            self.exclude_selected();
        }
        if ctx_pin {
            self.pin_selected();
        }
        if ctx_forget {
            if let Some(it) = self.results.get(self.selected).cloned() {
                self.engine.forget(&it);
                self.dirty = true;
            }
        }
    }

    fn draw_footer(&mut self, ui: &mut egui::Ui) {
        let p = self.palette;
        let (rect, _) = ui.allocate_exact_size(
            Vec2::new(ui.available_width(), FOOTER_H - 6.0),
            Sense::hover(),
        );
        let painter = ui.painter();
        painter.line_segment(
            [
                Pos2::new(rect.left() + 6.0, rect.top()),
                Pos2::new(rect.right() - 6.0, rect.top()),
            ],
            Stroke::new(1.0, p.border.gamma_multiply(0.6)),
        );
        let (status, col) = if let Some((msg, t)) = &self.toast {
            if t.elapsed() < Duration::from_secs(3) {
                (msg.clone(), p.accent)
            } else {
                (String::new(), p.dim)
            }
        } else {
            (String::new(), p.dim)
        };
        let (status, col) = if !status.is_empty() {
            (status, col)
        } else if let Some(e) = &self.ev_error {
            (
                format!("⚠ {}", e.message()),
                Color32::from_rgb(255, 170, 80),
            )
        } else if self.ev_ok {
            (
                "Everything 已连接".to_string(),
                Color32::from_rgb(90, 200, 140),
            )
        } else {
            (String::new(), p.dim)
        };
        painter.text(
            Pos2::new(rect.left() + 10.0, rect.center().y + 2.0),
            Align2::LEFT_CENTER,
            status,
            FontId::proportional(12.0),
            col,
        );
        let nav = crate::pathnav::is_path_query(&self.query);
        let hint = if self.dialog {
            "文件对话框模式 · Enter 跳转到该文件夹 (选文件则填入文件名)"
        } else {
            ""
        };
        let hint = if !hint.is_empty() { hint } else { match self.results.get(self.selected).map(|i| i.kind) {
            Some(Kind::File | Kind::Folder) if nav => "Tab/→ 补全 · Alt+← 上一级 · Enter 打开 · Shift+Enter 所在文件夹",
            Some(Kind::Snippet) => "Enter 粘贴到之前的窗口",
            Some(Kind::Clip) => "Enter 粘贴 · Shift+Enter 仅复制 · Ctrl+P 置顶 · Ctrl+Del 删除 · Alt+Del 删除全部匹配",
            Some(Kind::File | Kind::Folder) => {
                "Enter 打开 · Ctrl+Enter 管理员 · Shift+Enter 所在文件夹 · Ctrl+T 终端"
            }
            _ => "↑↓ 选择 · Enter 打开 · Ctrl+Enter 管理员 · Ctrl+, 设置",
        } };
        painter.text(
            Pos2::new(rect.right() - 10.0, rect.center().y + 2.0),
            Align2::RIGHT_CENTER,
            hint,
            FontId::proportional(11.5),
            p.dim,
        );
    }

    fn draw_panel(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let p = self.palette;
        ui.horizontal(|ui| {
            ui.heading("设置");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("关闭 (Esc)").clicked() {
                    self.panel_open = false;
                }
            });
        });
        ui.separator();
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.label(egui::RichText::new("Everything").strong());
            ui.horizontal(|ui| {
                ui.label("路径");
                ui.add(egui::TextEdit::singleline(&mut self.ui_everything_path).desired_width(420.0).hint_text("留空 = 自动检测 (Everything.exe)"));
                if ui.button("应用").clicked() {
                    self.engine.cfg.everything_path = self.ui_everything_path.trim().to_string();
                    if let Ok(mut g) = self.ev_path.lock() {
                        *g = self.engine.cfg.everything_path.clone();
                    }
                    self.engine.cfg.save();
                    self.toast("Everything 路径已保存");
                    self.sent_query.clear();
                }
            });
            match everything::find_exe(&self.ui_everything_path) {
                Some(path) => {
                    ui.colored_label(Color32::from_rgb(90, 200, 140), format!("已找到: {}", path.display()));
                }
                None => {
                    ui.colored_label(Color32::from_rgb(255, 170, 80), "未找到 Everything.exe，请填写完整路径");
                }
            }
            ui.add_space(8.0);

            ui.label(egui::RichText::new("快捷键与行为").strong());
            self.hotkey_row(ui, HotkeyTarget::Main);
            let mut changed = false;
            changed |= ui.checkbox(&mut self.engine.cfg.hide_on_blur, "失去焦点时自动隐藏 (设置面板打开时不会隐藏)").changed();
            ui.horizontal(|ui| {
                ui.label("按住窗口任意位置拖动即可移动（选文字、拖滑块除外），位置会被记住");
                if ui.button("重置窗口位置").clicked() {
                    self.engine.cfg.window_pos = None;
                    platform::recenter_window(&self.shared);
                    self.engine.cfg.save();
                }
            });
            changed |= ui.checkbox(&mut self.engine.cfg.dark_mode, "深色主题").changed();
            if ui.checkbox(&mut self.engine.cfg.show_recent_on_empty, "输入为空时显示常用项目 (默认关闭，只显示搜索框)").changed() {
                changed = true;
                self.dirty = true;
            }
            if ui.checkbox(&mut self.ui_autostart, "开机自启动").changed() {
                platform::set_autostart(self.ui_autostart);
            }
            ui.horizontal(|ui| {
                changed |= ui.checkbox(&mut self.engine.cfg.show_files, "搜索文件").changed();
                changed |= ui.checkbox(&mut self.engine.cfg.show_apps, "搜索应用").changed();
                changed |= ui.checkbox(&mut self.engine.cfg.show_bookmarks, "搜索浏览器书签").changed();
                changed |= ui.checkbox(&mut self.engine.cfg.show_settings, "搜索 Windows 设置").changed();
            });
            if changed {
                let dark = self.engine.cfg.dark_mode;
                self.palette = if dark { Palette::dark() } else { Palette::light() };
                apply_style(ctx, &self.palette, dark);
                self.engine.cfg.save();
                self.apps_changed(ctx);
            }
            ui.add_space(8.0);

            ui.label(egui::RichText::new("剪贴板历史").strong());
            self.hotkey_row(ui, HotkeyTarget::Clip);
            let mut clip_changed = false;
            clip_changed |= ui.checkbox(&mut self.engine.cfg.clipboard_enabled, "记录剪贴板历史 (保存在程序目录的 data 文件夹)").changed();
            clip_changed |= ui.checkbox(&mut self.engine.cfg.clipboard_images, "同时记录图片 (截图、复制的图片)").changed();
            ui.horizontal(|ui| {
                ui.label("最多保留");
                clip_changed |= ui.add(egui::DragValue::new(&mut self.engine.cfg.clipboard_max).range(100..=50_000).suffix(" 条")).changed();
                ui.label("最长保留");
                clip_changed |= ui.add(egui::DragValue::new(&mut self.engine.cfg.clipboard_days).range(0..=3650).suffix(" 天")).changed();
                ui.label(egui::RichText::new("(0 = 永久；置顶项不会被清理)").color(p.dim).size(12.0));
            });
            ui.horizontal(|ui| {
                ui.label("图片总量上限");
                clip_changed |= ui.add(egui::DragValue::new(&mut self.engine.cfg.clipboard_image_mb).range(0..=20_000).suffix(" MB")).changed();
                ui.label(egui::RichText::new("(0 = 不限制)").color(p.dim).size(12.0));
            });
            ui.horizontal(|ui| {
                ui.label("忽略这些程序");
                let r = ui.add(
                    egui::TextEdit::singleline(&mut self.ui_ignore_apps).desired_width(380.0).hint_text("例如 keepass.exe, 1password.exe (逗号分隔)"),
                );
                if r.lost_focus() {
                    self.engine.cfg.clipboard_ignore_apps =
                        self.ui_ignore_apps.split([',', ';', '\n']).map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).collect();
                    clip_changed = true;
                }
            });
            let (total, imgs, img_bytes) = self.engine.clip.stats();
            let db_size = std::fs::metadata(clipboard::db_path()).map(|m| m.len()).unwrap_or(0);
            ui.label(
                egui::RichText::new(format!(
                    "当前 {total} 条 (图片 {imgs} 张，{}) · 数据库 {}",
                    clipimg::human_bytes(img_bytes),
                    clipimg::human_bytes(db_size)
                ))
                .color(p.dim)
                .size(12.0),
            );
            ui.horizontal(|ui| {
                if ui.button("清空历史 (保留置顶项)").clicked() {
                    self.engine.clip.clear_unpinned();
                    self.thumbs.clear();
                    self.dirty = true;
                    self.toast("剪贴板历史已清空");
                }
                if ui.button("打开数据目录").clicked() {
                    let _ = platform::shell_open(&config::data_dir().to_string_lossy(), "", false, None);
                }
            });
            ui.label(
                egui::RichText::new("输入 cb 或按剪贴板快捷键打开；cb 后可加关键词、来源程序 (如 cb chrome)，或 img / text / pin 过滤。Enter 粘贴到之前的窗口。")
                    .color(p.dim)
                    .size(12.0),
            );
            if clip_changed {
                self.clip_settings.apply(&self.engine.cfg);
                self.engine.cfg.save();
            }
            ui.add_space(8.0);

            ui.label(egui::RichText::new("排除目录 (不出现在搜索结果中)").strong());
            let mut remove: Option<usize> = None;
            for (i, d) in self.engine.cfg.excluded_dirs.iter().enumerate() {
                ui.horizontal(|ui| {
                    if ui.small_button("✕").clicked() {
                        remove = Some(i);
                    }
                    ui.label(d);
                });
            }
            if let Some(i) = remove {
                self.engine.cfg.excluded_dirs.remove(i);
                self.engine.cfg.save();
                self.dirty = true;
            }
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut self.ui_new_exclude).desired_width(420.0).hint_text(r"例如 D:\Games"));
                if ui.button("添加").clicked() && !self.ui_new_exclude.trim().is_empty() {
                    let d = self.ui_new_exclude.trim().trim_matches('"').to_string();
                    self.engine.exclude(&d);
                    self.ui_new_exclude.clear();
                    self.dirty = true;
                }
            });
            ui.add_space(8.0);

            ui.label(egui::RichText::new(format!("自定义命令 ({})", self.engine.cfg.commands.len())).strong());
            ui.label(
                egui::RichText::new("输入触发词即可运行，例如 cmd = 以管理员身份打开命令提示符。命令、网页搜索都在配置文件里编辑，保存后点击“重新加载”。")
                    .color(p.dim)
                    .size(12.0),
            );
            ui.horizontal(|ui| {
                if ui.button("打开配置文件").clicked() {
                    let _ = platform::shell_open("notepad.exe", &format!("\"{}\"", Config::path().display()), false, None);
                }
                if ui.button("重新加载配置").clicked() {
                    let learner_keep = std::mem::take(&mut self.engine.learner);
                    let cfg = Config::load();
                    self.palette = if cfg.dark_mode { Palette::dark() } else { Palette::light() };
                    apply_style(ctx, &self.palette, cfg.dark_mode);
                    self.ui_everything_path = cfg.everything_path.clone();
                    self.engine.cfg = cfg;
                    self.engine.learner = learner_keep;
                    if let Ok(mut h) = self.shared.hotkey.lock() {
                        *h = self.engine.cfg.hotkey.clone();
                    }
                    if let Ok(mut h) = self.shared.clip_hotkey.lock() {
                        *h = self.engine.cfg.clipboard_hotkey.clone();
                    }
                    platform::reapply_hotkey(&self.shared);
                    self.clip_settings.apply(&self.engine.cfg);
                    self.ui_ignore_apps = self.engine.cfg.clipboard_ignore_apps.join(", ");
                    self.apps_changed(ctx);
                    self.toast("配置已重新加载");
                }
                if ui.button("重建应用索引").clicked() {
                    self.start_scan(ctx);
                    self.toast("正在重建应用索引…");
                }
            });
            ui.add_space(8.0);

            ui.label(egui::RichText::new("自学习排序").strong());
            ui.label(egui::RichText::new(format!("已记住 {} 个常用项。选中过的项目会在下次同样的输入中排到更前面。", self.engine.learner.len())).color(p.dim).size(12.0));
            if ui.button("清除全部学习记录").clicked() {
                self.engine.learner = Learner::load(config::data_dir().join("learn.json.cleared"));
                let _ = std::fs::remove_file(config::data_dir().join("learn.json"));
                self.engine.learner = Learner::load(config::data_dir().join("learn.json"));
                self.toast("学习记录已清除");
                self.dirty = true;
            }
        });
    }

    fn hotkey_row(&mut self, ui: &mut egui::Ui, target: HotkeyTarget) {
        let p = self.palette;
        let (label, current, default) = match target {
            HotkeyTarget::Main => ("呼出快捷键", self.engine.cfg.hotkey.clone(), "Alt+Space"),
            HotkeyTarget::Clip => (
                "剪贴板快捷键",
                self.engine.cfg.clipboard_hotkey.clone(),
                "Ctrl+Alt+V",
            ),
        };
        let recording = self.recording == Some(target);
        ui.horizontal(|ui| {
            ui.label(label);
            let text = if recording {
                "请按下新的快捷键…  (Esc 取消)".to_string()
            } else if current.is_empty() {
                "未设置 (点击录制)".to_string()
            } else {
                format!("{current}   (点击修改)")
            };
            let mut btn = egui::Button::new(egui::RichText::new(text).size(14.0))
                .min_size(Vec2::new(250.0, 28.0));
            if recording {
                btn = btn.stroke(Stroke::new(1.5, p.accent));
            }
            if ui.add(btn).clicked() {
                if recording {
                    self.stop_recording();
                } else {
                    self.start_recording(target);
                }
            }
            if recording {
                ui.checkbox(&mut self.rec_win, "含 Win 键");
            } else {
                if current != default && ui.button("恢复默认").clicked() {
                    self.set_hotkey(target, default.to_string());
                }
                if target == HotkeyTarget::Clip
                    && !current.is_empty()
                    && ui.button("关闭").clicked()
                {
                    self.set_hotkey(target, String::new());
                }
            }
        });
        let err = match target {
            HotkeyTarget::Main => self.shared.hotkey_error.lock().ok().and_then(|g| g.clone()),
            HotkeyTarget::Clip => self
                .shared
                .clip_hotkey_error
                .lock()
                .ok()
                .and_then(|g| g.clone()),
        };
        if let Some(e) = err {
            ui.colored_label(Color32::from_rgb(255, 120, 100), format!("⚠ {e}"));
        }
    }

    fn apps_changed(&mut self, ctx: &egui::Context) {
        let cached = apps::load_cache();
        self.engine.rebuild(&cached);
        self.dirty = true;
        let _ = ctx;
    }
}

impl eframe::App for App {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn ui(&mut self, root: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = &root.ctx().clone();
        // First frame: center the window if we are starting visible.
        if !self.positioned {
            self.positioned = true;
            if !self.background {
                platform::show_window(&self.shared);
            }
        }
        if self.shared.just_shown.swap(false, Ordering::SeqCst) {
            self.on_shown(ctx);
        }

        self.poll_channels();
        if self.clip_changed.swap(false, Ordering::SeqCst) && self.is_clip_query(&self.query) {
            self.dirty = true;
        }
        // Never leave the global hotkeys released if the settings panel went away mid-recording.
        if self.recording.is_some()
            && (!self.panel_open || !self.shared.visible.load(Ordering::SeqCst))
        {
            self.stop_recording();
        }
        // Tell the user once if a hotkey could not be registered (e.g. taken by another program).
        if !self.hotkey_warned && self.shared.visible.load(Ordering::SeqCst) {
            let e = self
                .shared
                .hotkey_error
                .lock()
                .ok()
                .and_then(|g| g.clone())
                .or_else(|| {
                    self.shared
                        .clip_hotkey_error
                        .lock()
                        .ok()
                        .and_then(|g| g.clone())
                });
            if let Some(e) = e {
                self.toast(format!("⚠ {e} (设置 Ctrl+, 中可更换)"));
                self.hotkey_warned = true;
            }
        }
        self.track_drag(ctx);
        self.poll_icons(ctx);
        self.handle_keys(ctx);
        self.send_search_if_needed();
        if self.dirty {
            self.recompute();
        }

        // Auto-hide when focus is lost.
        let focused = ctx.input(|i| i.viewport().focused.unwrap_or(true));
        if focused {
            self.was_focused = true;
        } else if self.was_focused
            && self.engine.cfg.hide_on_blur
            && !self.panel_open // settings stay open while you copy a path from another window
            && self.shared.visible.load(Ordering::SeqCst)
        {
            self.hide();
        }

        // Fit window height to content.
        let h = self.wanted_height();
        if (h - self.last_height).abs() > 0.5 {
            self.last_height = h;
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(Vec2::new(WIDTH, h)));
        }

        let p = self.palette;
        let frame = egui::Frame::NONE
            .fill(p.bg)
            .stroke(Stroke::new(1.0, p.border))
            .corner_radius(CornerRadius::same(16))
            .inner_margin(egui::Margin::same(PAD as i8));
        egui::CentralPanel::default().frame(frame).show(root, |ui| {
            if self.panel_open {
                self.draw_panel(ui, ctx);
            } else {
                self.draw_header(ui);
                if !self.results.is_empty() || !self.query.trim().is_empty() {
                    ui.add_space(4.0);
                    self.draw_list(ui, ctx);
                    ui.add_space(2.0);
                    self.draw_footer(ui);
                }
            }
        });

        self.window_drag(ctx);

        if let Some((_, t)) = &self.toast {
            if t.elapsed() < Duration::from_secs(3) {
                ctx.request_repaint_after(Duration::from_millis(500));
            }
        }
    }
}

// ───────────────────────────── text helpers ─────────────────────────────

fn ellipsize_middle(s: &str, max: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max {
        return s.to_string();
    }
    let head = max / 3;
    let tail = max - head - 1;
    let mut out: String = chars[..head].iter().collect();
    out.push('…');
    out.extend(chars[chars.len() - tail..].iter());
    out
}

/// Build a text layout where characters matched by the query tokens use the accent color.
fn highlight_job(
    text: &str,
    query: &str,
    font: FontId,
    base: Color32,
    accent: Color32,
) -> egui::text::LayoutJob {
    let chars: Vec<char> = text.chars().collect();
    let lower: Vec<char> = chars
        .iter()
        .map(|c| c.to_lowercase().next().unwrap_or(*c))
        .collect();
    let mut mask = vec![false; chars.len()];
    for tok in query.split_whitespace() {
        let t: Vec<char> = tok
            .chars()
            .map(|c| c.to_lowercase().next().unwrap_or(c))
            .collect();
        if t.is_empty() || t.len() > lower.len() {
            continue;
        }
        let mut i = 0;
        while i + t.len() <= lower.len() {
            if lower[i..i + t.len()] == t[..] {
                mask[i..i + t.len()].iter_mut().for_each(|m| *m = true);
                i += t.len();
            } else {
                i += 1;
            }
        }
    }
    let mut job = egui::text::LayoutJob::default();
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    let mut start = 0;
    while start < chars.len() {
        let m = mask[start];
        let mut end = start;
        while end < chars.len() && mask[end] == m {
            end += 1;
        }
        let seg: String = chars[start..end].iter().collect();
        job.append(
            &seg,
            0.0,
            egui::TextFormat {
                font_id: font.clone(),
                color: if m { accent } else { base },
                ..Default::default()
            },
        );
        start = end;
    }
    job
}
