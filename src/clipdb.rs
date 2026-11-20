//! Clipboard history storage on **redb**, a pure-Rust embedded database (no C compiler, no DLL).
//!
//! * text and images (PNG + small thumbnail), de-duplicated by content hash
//! * newest first, pinned items always on top and exempt from clean-up
//! * substring search over text and source program, plus `img` / `text` / `pin` filters
//! * clean-up by item count, age and total image size
//!
//! Layout: small metadata rows live in one table, while text, PNG and thumbnail blobs live in
//! their own tables, so listing and filtering never has to read image data. `Store` is a cheap
//! `Clone` (shared handle): redb allows one open handle per file, so the UI and the clipboard
//! watcher thread share it. Timestamps are Unix **milliseconds**.

use redb::{Database, ReadableDatabase, ReadableTable, TableDefinition};
use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;

pub const MAX_TEXT_CHARS: usize = 200_000;
pub const MAX_IMAGE_BYTES: usize = 30 * 1024 * 1024;

pub type DbResult<T> = Result<T, redb::Error>;

const META: TableDefinition<u64, &[u8]> = TableDefinition::new("meta");
const TEXTS: TableDefinition<u64, &str> = TableDefinition::new("texts");
const PNGS: TableDefinition<u64, &[u8]> = TableDefinition::new("pngs");
const THUMBS: TableDefinition<u64, &[u8]> = TableDefinition::new("thumbs");
const TEXT_HASH: TableDefinition<u64, u64> = TableDefinition::new("text_hash");
const IMG_HASH: TableDefinition<u64, u64> = TableDefinition::new("img_hash");
const SEQ: TableDefinition<&str, u64> = TableDefinition::new("seq");

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClipKind {
    Text,
    Image,
}

/// One list row. Text is truncated to 400 characters (enough for any preview).
#[derive(Clone, Debug)]
pub struct ClipRow {
    pub id: i64,
    pub kind: ClipKind,
    pub preview: String,
    pub chars: u64,
    pub lines: u64,
    pub ts_ms: u64,
    pub pinned: bool,
    pub src: String,
    pub w: u32,
    pub h: u32,
    pub bytes: u64,
}

/// What gets put back on the clipboard.
#[derive(Clone, Debug, PartialEq)]
pub enum ClipContent {
    Text(String),
    Png(Vec<u8>),
}

/// FNV-1a 64-bit. Stable across Rust versions (unlike `DefaultHasher`), which matters because
/// hashes are stored in the database.
pub fn content_hash(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// Row metadata, stored as a compact binary record.
#[derive(Clone, Debug, PartialEq)]
struct Meta {
    kind: ClipKind,
    pinned: bool,
    hash: u64,
    ts: u64,
    w: u32,
    h: u32,
    bytes: u64,
    src: String,
}

const META_FIXED: usize = 2 + 8 + 8 + 4 + 4 + 8;

impl Meta {
    fn encode(&self) -> Vec<u8> {
        let mut v = Vec::with_capacity(META_FIXED + self.src.len());
        v.push(if self.kind == ClipKind::Image { 1 } else { 0 });
        v.push(self.pinned as u8);
        v.extend_from_slice(&self.hash.to_le_bytes());
        v.extend_from_slice(&self.ts.to_le_bytes());
        v.extend_from_slice(&self.w.to_le_bytes());
        v.extend_from_slice(&self.h.to_le_bytes());
        v.extend_from_slice(&self.bytes.to_le_bytes());
        v.extend_from_slice(self.src.as_bytes());
        v
    }

    fn decode(b: &[u8]) -> Option<Meta> {
        if b.len() < META_FIXED {
            return None;
        }
        let u64_at = |o: usize| u64::from_le_bytes(b[o..o + 8].try_into().unwrap());
        let u32_at = |o: usize| u32::from_le_bytes(b[o..o + 4].try_into().unwrap());
        Some(Meta {
            kind: if b[0] == 1 {
                ClipKind::Image
            } else {
                ClipKind::Text
            },
            pinned: b[1] != 0,
            hash: u64_at(2),
            ts: u64_at(10),
            w: u32_at(18),
            h: u32_at(22),
            bytes: u64_at(26),
            src: String::from_utf8_lossy(&b[META_FIXED..]).into_owned(),
        })
    }
}

/// Shared handle to the clipboard database.
#[derive(Clone)]
pub struct Store {
    db: Arc<Database>,
}

impl Store {
    /// Open (or create) the database file. The file is compacted on open, so space freed by
    /// earlier clean-ups is returned to the OS.
    pub fn open(path: &Path) -> DbResult<Self> {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        // Small page cache: this app lives in the background and memory matters.
        let mut db = Database::builder()
            .set_cache_size(8 * 1024 * 1024)
            .create(path)?;
        let _ = db.compact(redb::CompactionMode::Balanced);
        Self::init(db)
    }

    pub fn open_memory() -> DbResult<Self> {
        let db = Database::builder().create_with_backend(redb::backends::InMemoryBackend::new())?;
        Self::init(db)
    }

    fn init(db: Database) -> DbResult<Self> {
        // Create every table up front so read transactions never see a missing table.
        let tx = db.begin_write()?;
        {
            tx.open_table(META)?;
            tx.open_table(TEXTS)?;
            tx.open_table(PNGS)?;
            tx.open_table(THUMBS)?;
            tx.open_table(TEXT_HASH)?;
            tx.open_table(IMG_HASH)?;
            tx.open_table(SEQ)?;
        }
        tx.commit()?;
        Ok(Self { db: Arc::new(db) })
    }

    fn next_id(tx: &redb::WriteTransaction) -> DbResult<u64> {
        let mut seq = tx.open_table(SEQ)?;
        let id = seq.get("id")?.map(|g| g.value()).unwrap_or(0) + 1;
        seq.insert("id", id)?;
        Ok(id)
    }

    /// If `hash` already exists for this kind, refresh its time (and source) and return its id.
    fn bump_existing(
        tx: &redb::WriteTransaction,
        image: bool,
        hash: u64,
        src: &str,
        now: u64,
    ) -> DbResult<Option<u64>> {
        let existing = {
            let hashes = tx.open_table(if image { IMG_HASH } else { TEXT_HASH })?;
            let id = hashes.get(hash)?.map(|g| g.value());
            id
        };
        let Some(id) = existing else { return Ok(None) };
        let mut meta = tx.open_table(META)?;
        let old = meta.get(id)?.and_then(|g| Meta::decode(g.value()));
        let Some(mut m) = old else { return Ok(None) }; // dangling hash entry: caller re-inserts
        m.ts = now;
        if !src.is_empty() {
            m.src = src.to_string();
        }
        meta.insert(id, m.encode().as_slice())?;
        Ok(Some(id))
    }

    /// Insert text, or move an identical older clip to the top. `src` is the program it was
    /// copied from (empty = unknown; an existing source is kept in that case).
    pub fn add_text(&self, text: &str, src: &str, now_ms: u64) -> DbResult<Option<i64>> {
        if text.trim().is_empty() || text.chars().nth(MAX_TEXT_CHARS).is_some() {
            return Ok(None);
        }
        let hash = content_hash(text.as_bytes());
        let tx = self.db.begin_write()?;
        let id = match Self::bump_existing(&tx, false, hash, src, now_ms)? {
            Some(id) => id,
            None => {
                let id = Self::next_id(&tx)?;
                let m = Meta {
                    kind: ClipKind::Text,
                    pinned: false,
                    hash,
                    ts: now_ms,
                    w: 0,
                    h: 0,
                    bytes: text.len() as u64,
                    src: src.to_string(),
                };
                tx.open_table(TEXTS)?.insert(id, text)?;
                tx.open_table(META)?.insert(id, m.encode().as_slice())?;
                tx.open_table(TEXT_HASH)?.insert(hash, id)?;
                id
            }
        };
        tx.commit()?;
        Ok(Some(id as i64))
    }

    pub fn add_image(
        &self,
        png: &[u8],
        thumb: &[u8],
        w: u32,
        h: u32,
        src: &str,
        now_ms: u64,
    ) -> DbResult<Option<i64>> {
        if png.is_empty() || png.len() > MAX_IMAGE_BYTES || w == 0 || h == 0 {
            return Ok(None);
        }
        let hash = content_hash(png);
        let tx = self.db.begin_write()?;
        let id = match Self::bump_existing(&tx, true, hash, src, now_ms)? {
            Some(id) => id,
            None => {
                let id = Self::next_id(&tx)?;
                let m = Meta {
                    kind: ClipKind::Image,
                    pinned: false,
                    hash,
                    ts: now_ms,
                    w,
                    h,
                    bytes: (png.len() + thumb.len()) as u64,
                    src: src.to_string(),
                };
                tx.open_table(PNGS)?.insert(id, png)?;
                tx.open_table(THUMBS)?.insert(id, thumb)?;
                tx.open_table(META)?.insert(id, m.encode().as_slice())?;
                tx.open_table(IMG_HASH)?.insert(hash, id)?;
                id
            }
        };
        tx.commit()?;
        Ok(Some(id as i64))
    }

    /// Every metadata row (unsorted).
    fn all_meta(&self) -> Vec<(u64, Meta)> {
        let Ok(rtx) = self.db.begin_read() else {
            return Vec::new();
        };
        let Ok(meta) = rtx.open_table(META) else {
            return Vec::new();
        };
        let Ok(iter) = meta.iter() else {
            return Vec::new();
        };
        iter.flatten()
            .filter_map(|(k, v)| Meta::decode(v.value()).map(|m| (k.value(), m)))
            .collect()
    }

    /// Search. Whitespace-separated tokens are ANDed; each must occur in the text or in the
    /// source program name (case-insensitive). Special tokens: `img`/`图片` (images only),
    /// `text`/`文本`, `pin`/`置顶`. Newest first, pinned on top.
    pub fn search(&self, query: &str, limit: usize) -> Vec<ClipRow> {
        let mut only_kind: Option<ClipKind> = None;
        let mut only_pinned = false;
        let mut tokens: Vec<String> = Vec::new();
        for tok in query.split_whitespace() {
            match tok.to_lowercase().as_str() {
                "img" | "image" | "图" | "图片" | "截图" => only_kind = Some(ClipKind::Image),
                "text" | "文本" | "文字" => only_kind = Some(ClipKind::Text),
                "pin" | "pinned" | "置顶" | "收藏" => only_pinned = true,
                t => tokens.push(t.to_string()),
            }
        }
        let Ok(rtx) = self.db.begin_read() else {
            return Vec::new();
        };
        let (Ok(meta), Ok(texts)) = (rtx.open_table(META), rtx.open_table(TEXTS)) else {
            return Vec::new();
        };
        let Ok(iter) = meta.iter() else {
            return Vec::new();
        };

        let mut hits: Vec<(u64, Meta)> = Vec::new();
        for (k, v) in iter.flatten() {
            let Some(m) = Meta::decode(v.value()) else {
                continue;
            };
            if only_kind.is_some_and(|k| k != m.kind) || (only_pinned && !m.pinned) {
                continue;
            }
            let id = k.value();
            if !tokens.is_empty() {
                let src_lc = m.src.to_lowercase();
                let text_lc = if m.kind == ClipKind::Text {
                    texts
                        .get(id)
                        .ok()
                        .flatten()
                        .map(|g| g.value().to_lowercase())
                } else {
                    None
                };
                let all = tokens.iter().all(|t| {
                    src_lc.contains(t.as_str())
                        || text_lc.as_deref().is_some_and(|x| x.contains(t.as_str()))
                });
                if !all {
                    continue;
                }
            }
            hits.push((id, m));
        }
        hits.sort_by(|a, b| {
            b.1.pinned
                .cmp(&a.1.pinned)
                .then(b.1.ts.cmp(&a.1.ts))
                .then(b.0.cmp(&a.0))
        });
        hits.truncate(limit.min(500));

        hits.into_iter()
            .map(|(id, m)| {
                let (preview, chars, lines) = match m.kind {
                    ClipKind::Text => match texts.get(id).ok().flatten() {
                        Some(g) => {
                            let t = g.value();
                            (
                                t.chars().take(400).collect(),
                                t.chars().count() as u64,
                                t.matches('\n').count() as u64 + 1,
                            )
                        }
                        None => (String::new(), 0, 0),
                    },
                    ClipKind::Image => (String::new(), 0, 0),
                };
                ClipRow {
                    id: id as i64,
                    kind: m.kind,
                    preview,
                    chars,
                    lines,
                    ts_ms: m.ts,
                    pinned: m.pinned,
                    src: m.src,
                    w: m.w,
                    h: m.h,
                    bytes: m.bytes,
                }
            })
            .collect()
    }

    fn read_blob(&self, def: TableDefinition<u64, &[u8]>, id: i64) -> Option<Vec<u8>> {
        let rtx = self.db.begin_read().ok()?;
        let t = rtx.open_table(def).ok()?;
        let v = t.get(id as u64).ok()??.value().to_vec();
        Some(v)
    }

    pub fn get_text(&self, id: i64) -> Option<String> {
        let rtx = self.db.begin_read().ok()?;
        let t = rtx.open_table(TEXTS).ok()?;
        let v = t.get(id as u64).ok()??.value().to_string();
        Some(v)
    }

    pub fn get_png(&self, id: i64) -> Option<Vec<u8>> {
        self.read_blob(PNGS, id)
    }

    pub fn get_thumb(&self, id: i64) -> Option<Vec<u8>> {
        self.read_blob(THUMBS, id)
    }

    pub fn content(&self, id: i64, image: bool) -> Option<ClipContent> {
        if image {
            self.get_png(id).map(ClipContent::Png)
        } else {
            self.get_text(id).map(ClipContent::Text)
        }
    }

    /// Remove rows (metadata, blobs and hash entries). With `only_unpinned`, pinned rows are kept.
    fn remove_ids(&self, ids: &[u64], only_unpinned: bool) -> usize {
        if ids.is_empty() {
            return 0;
        }
        let run = || -> DbResult<usize> {
            let tx = self.db.begin_write()?;
            let mut removed = 0;
            {
                let mut meta = tx.open_table(META)?;
                let mut texts = tx.open_table(TEXTS)?;
                let mut pngs = tx.open_table(PNGS)?;
                let mut thumbs = tx.open_table(THUMBS)?;
                let mut text_hash = tx.open_table(TEXT_HASH)?;
                let mut img_hash = tx.open_table(IMG_HASH)?;
                for &id in ids {
                    let m = meta.get(id)?.and_then(|g| Meta::decode(g.value()));
                    let Some(m) = m else { continue };
                    if only_unpinned && m.pinned {
                        continue;
                    }
                    meta.remove(id)?;
                    match m.kind {
                        ClipKind::Text => {
                            texts.remove(id)?;
                            text_hash.remove(m.hash)?;
                        }
                        ClipKind::Image => {
                            pngs.remove(id)?;
                            thumbs.remove(id)?;
                            img_hash.remove(m.hash)?;
                        }
                    }
                    removed += 1;
                }
            }
            tx.commit()?;
            Ok(removed)
        };
        run().unwrap_or(0)
    }

    pub fn delete(&self, id: i64) {
        self.remove_ids(&[id as u64], false);
    }

    /// Flip the pin flag; returns the new state.
    pub fn toggle_pin(&self, id: i64) -> Option<bool> {
        let run = || -> DbResult<Option<bool>> {
            let tx = self.db.begin_write()?;
            let result = {
                let mut meta = tx.open_table(META)?;
                let old = meta.get(id as u64)?.and_then(|g| Meta::decode(g.value()));
                match old {
                    Some(mut m) => {
                        m.pinned = !m.pinned;
                        meta.insert(id as u64, m.encode().as_slice())?;
                        Some(m.pinned)
                    }
                    None => None,
                }
            };
            tx.commit()?;
            Ok(result)
        };
        run().ok().flatten()
    }

    pub fn clear_unpinned(&self) {
        let ids: Vec<u64> = self
            .all_meta()
            .into_iter()
            .filter(|(_, m)| !m.pinned)
            .map(|(id, _)| id)
            .collect();
        self.remove_ids(&ids, true);
    }

    /// (total items, image items, bytes used by images)
    pub fn stats(&self) -> (u64, u64, u64) {
        let mut s = (0, 0, 0);
        for (_, m) in self.all_meta() {
            s.0 += 1;
            if m.kind == ClipKind::Image {
                s.1 += 1;
                s.2 += m.bytes;
            }
        }
        s
    }

    /// Delete old items (never pinned ones). `max_days == 0` keeps items forever;
    /// `image_cap_bytes == 0` means no image size limit. Returns how many rows were removed.
    pub fn prune(
        &self,
        max_items: usize,
        max_days: u32,
        image_cap_bytes: u64,
        now_ms: u64,
    ) -> usize {
        let mut metas = self.all_meta();
        let mut doomed: HashSet<u64> = HashSet::new();

        if max_days > 0 {
            let cutoff = now_ms.saturating_sub(max_days as u64 * 86_400_000);
            doomed.extend(
                metas
                    .iter()
                    .filter(|(_, m)| !m.pinned && m.ts < cutoff)
                    .map(|(id, _)| *id),
            );
        }
        // Newest first, so "keep the first N" and the running image total are simple scans.
        metas.sort_by(|a, b| b.1.ts.cmp(&a.1.ts).then(b.0.cmp(&a.0)));
        let mut kept_unpinned = 0usize;
        for (id, m) in &metas {
            if m.pinned || doomed.contains(id) {
                continue;
            }
            kept_unpinned += 1;
            if kept_unpinned > max_items {
                doomed.insert(*id);
            }
        }
        if image_cap_bytes > 0 {
            let mut run = 0u64;
            let imgs: Vec<(u64, bool, u64)> = metas
                .iter()
                .filter(|(id, m)| m.kind == ClipKind::Image && !doomed.contains(id))
                .map(|(id, m)| (*id, m.pinned, m.bytes))
                .collect();
            for (id, pinned, bytes) in imgs {
                run += bytes;
                if run > image_cap_bytes && !pinned {
                    doomed.insert(id);
                }
            }
        }
        let ids: Vec<u64> = doomed.into_iter().collect();
        self.remove_ids(&ids, true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn st() -> Store {
        Store::open_memory().unwrap()
    }
    fn texts(rows: &[ClipRow]) -> Vec<String> {
        rows.iter().map(|r| r.preview.clone()).collect()
    }

    #[test]
    fn meta_roundtrip_and_garbage() {
        let m = Meta {
            kind: ClipKind::Image,
            pinned: true,
            hash: u64::MAX,
            ts: 123,
            w: 4,
            h: 5,
            bytes: 6,
            src: "chrome.exe".into(),
        };
        assert_eq!(Meta::decode(&m.encode()), Some(m));
        assert!(Meta::decode(&[1, 2, 3]).is_none());
    }

    #[test]
    fn duplicate_moves_to_top_and_keeps_source() {
        let s = st();
        s.add_text("a", "chrome.exe", 1).unwrap();
        s.add_text("b", "code.exe", 2).unwrap();
        s.add_text("a", "", 3).unwrap(); // re-copied from an unknown program
        let rows = s.search("", 10);
        assert_eq!(texts(&rows), ["a", "b"]);
        assert_eq!(rows[0].src, "chrome.exe");
        assert_eq!(s.stats().0, 2);
    }

    #[test]
    fn rejects_blank_and_huge_text() {
        let s = st();
        assert_eq!(s.add_text("  \n\t", "", 1).unwrap(), None);
        assert_eq!(
            s.add_text(&"x".repeat(MAX_TEXT_CHARS + 1), "", 1).unwrap(),
            None
        );
        assert!(s
            .add_text(&"x".repeat(MAX_TEXT_CHARS), "", 1)
            .unwrap()
            .is_some());
    }

    #[test]
    fn search_is_and_case_insensitive_cjk_and_matches_source() {
        let s = st();
        s.add_text("Hello World", "notepad.exe", 1).unwrap();
        s.add_text("会议记录 2026 年度计划", "wechat.exe", 2)
            .unwrap();
        s.add_text("100% done_ok", "code.exe", 3).unwrap();
        assert_eq!(s.search("hello WORLD", 10).len(), 1);
        assert_eq!(s.search("hello nothere", 10).len(), 0);
        assert_eq!(
            texts(&s.search("会议 计划", 10)),
            ["会议记录 2026 年度计划"]
        );
        assert_eq!(s.search("wechat", 10).len(), 1); // by source program
        assert_eq!(s.search("100%", 10).len(), 1);
        assert_eq!(s.search("%", 10).len(), 1);
        assert_eq!(s.search("done_ok", 10).len(), 1);
        assert_eq!(s.search("_", 10).len(), 1);
        // A token can match the text of one word and the source of another.
        assert_eq!(s.search("hello notepad", 10).len(), 1);
    }

    #[test]
    fn row_metadata() {
        let s = st();
        s.add_text("line1\nline2\nline3", "", 1).unwrap();
        s.add_text("你好", "", 2).unwrap();
        let rows = s.search("", 10);
        assert_eq!((rows[0].chars, rows[0].lines), (2, 1));
        assert_eq!((rows[1].chars, rows[1].lines), (17, 3));
    }

    #[test]
    fn long_text_preview_is_truncated() {
        let s = st();
        s.add_text(&"é".repeat(1000), "", 1).unwrap();
        let r = &s.search("", 1)[0];
        assert_eq!((r.preview.chars().count(), r.chars), (400, 1000));
    }

    #[test]
    fn images_store_dedupe_and_filter() {
        let s = st();
        let id = s
            .add_image(&[1, 2, 3, 4], &[9, 9], 10, 20, "snip.exe", 1)
            .unwrap()
            .unwrap();
        s.add_text("txt", "", 2).unwrap();
        assert_eq!(
            s.add_image(&[1, 2, 3, 4], &[9, 9], 10, 20, "", 3).unwrap(),
            Some(id)
        );
        assert_eq!(s.stats(), (2, 1, 6));
        assert_eq!(s.get_png(id).unwrap(), vec![1, 2, 3, 4]);
        assert_eq!(s.get_thumb(id).unwrap(), vec![9, 9]);
        let imgs = s.search("img", 10);
        assert_eq!(imgs.len(), 1);
        assert_eq!(
            (imgs[0].kind, imgs[0].w, imgs[0].h),
            (ClipKind::Image, 10, 20)
        );
        assert_eq!(imgs[0].src, "snip.exe");
        assert_eq!(s.search("图片", 10).len(), 1);
        assert_eq!(s.search("text", 10).len(), 1);
        assert_eq!(s.search("snip", 10).len(), 1);
        assert_eq!(
            s.content(id, true),
            Some(ClipContent::Png(vec![1, 2, 3, 4]))
        );
        assert!(s.get_text(id).is_none());
        assert!(s.add_image(&[], &[], 1, 1, "", 1).unwrap().is_none());
    }

    #[test]
    fn pinned_items_sort_first_and_survive_cleanup() {
        let s = st();
        let a = s.add_text("old", "", 1).unwrap().unwrap();
        s.add_text("new", "", 1_000_000_000).unwrap();
        assert_eq!(s.toggle_pin(a), Some(true));
        assert_eq!(texts(&s.search("", 10)), ["old", "new"]);
        assert_eq!(texts(&s.search("pin", 10)), ["old"]);
        s.prune(1, 1, 0, 2_000_000_000); // both are "old" by age; only the pin survives
        assert_eq!(texts(&s.search("", 10)), ["old"]);
        s.clear_unpinned();
        assert_eq!(s.stats().0, 1);
        assert_eq!(s.toggle_pin(a), Some(false));
        s.delete(a);
        assert_eq!(s.stats().0, 0);
        assert_eq!(s.toggle_pin(a), None); // gone
    }

    #[test]
    fn delete_frees_hash_so_content_can_be_added_again() {
        let s = st();
        let a = s.add_text("again", "", 1).unwrap().unwrap();
        s.delete(a);
        let b = s.add_text("again", "", 2).unwrap().unwrap();
        assert_ne!(a, b); // ids are never reused
        assert_eq!(s.search("again", 5).len(), 1);
        assert_eq!(s.get_text(a), None);
    }

    #[test]
    fn prune_by_count_age_and_image_size() {
        let s = st();
        for i in 0..10u64 {
            s.add_text(&format!("t{i}"), "", i + 1).unwrap();
        }
        assert_eq!(s.prune(4, 0, 0, 100), 6);
        assert_eq!(texts(&s.search("", 10)), ["t9", "t8", "t7", "t6"]);

        // Age: items older than 1 day go.
        let day = 86_400_000u64;
        assert_eq!(s.prune(100, 1, 0, 10 * day), 4);

        // Image cap: newest images are kept until the total exceeds the cap.
        let s = st();
        for i in 0..5u8 {
            s.add_image(&[i; 100], &[], 1, 1, "", i as u64 + 1).unwrap();
        }
        assert_eq!(s.prune(100, 0, 250, 10), 3); // 2 newest (200 bytes) fit, 3 oldest removed
        assert_eq!(s.stats().1, 2);
    }

    #[test]
    fn data_survives_reopen_on_disk_and_handles_are_shared() {
        let dir = std::env::temp_dir().join(format!("sf_clipdb_{}", std::process::id()));
        let path = dir.join("clip.redb");
        {
            let s = Store::open(&path).unwrap();
            s.add_text("persist me 你好", "x.exe", 5).unwrap();
            s.add_image(&[7; 50], &[1], 3, 3, "", 6).unwrap();
        }
        let s2 = Store::open(&path).unwrap();
        assert_eq!(texts(&s2.search("persist", 5)), ["persist me 你好"]);
        assert_eq!(s2.search("img", 5).len(), 1);
        // A clone (like the one held by the watcher thread) sees writes immediately.
        let s3 = s2.clone();
        let t = std::thread::spawn(move || s3.add_text("from other thread", "", 7).unwrap());
        t.join().unwrap();
        assert_eq!(s2.search("other", 5).len(), 1);
        drop(s2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn hash_is_stable() {
        // Guards against accidental changes that would orphan stored hashes.
        assert_eq!(content_hash(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(content_hash(b"a"), 0xaf63_dc4c_8601_ec8c);
    }
}
