//! Self-learning ranking.
//!
//! Every time the user launches a result we remember (a) how often and how recently the item
//! was used, and (b) which query text was typed when it was picked. Both feed a score boost
//! that is added on top of the text-match score, so a picked item rises quickly for the same
//! (or a prefix of the same) query and slowly for everything else.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

const MAX_QUERIES_PER_ENTRY: usize = 6;
const MAX_ENTRIES: usize = 4000;
/// Usage fades with this half-life, so what you use *now* outranks what you used long ago.
const HALF_LIFE_DAYS: f64 = 30.0;
/// The query-independent "frequently used" boost never exceeds this (a text match must still
/// matter: a favourite app should not outrank a clearly better match for an unrelated query).
const FREQ_CAP: f32 = 42.0;
/// Upper limit of the query-association boost ("you picked this for exactly this text").
const ASSOC_CAP: f32 = 100.0;

#[derive(Serialize, Deserialize, Default, Clone, Debug)]
pub struct Entry {
    /// Total launches (kept for display/legacy; ranking uses `s`).
    pub count: u32,
    /// Time-decayed launch score ("frecency"): on every launch `s = s * 0.5^(days/half-life) + 1`.
    /// Missing in files written by older versions; then `count` is used.
    #[serde(default)]
    pub s: f32,
    pub last: u64,
    #[serde(default)]
    pub q: HashMap<String, u32>,
    /// Queries for which this item was shown above the user's real pick ("skipped").
    #[serde(default)]
    pub sk: HashMap<String, u32>,
    /// Original-case path for files/folders, so they can be offered even when Everything's
    /// result window does not include them.
    #[serde(default)]
    pub p: Option<String>,
}

#[derive(Default)]
pub struct Learner {
    map: HashMap<String, Entry>,
    path: Option<PathBuf>,
    dirty: bool,
}

pub fn normalize_query(q: &str) -> String {
    q.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

impl Learner {
    pub fn load(path: PathBuf) -> Self {
        let map = match std::fs::read(&path) {
            Ok(b) => serde_json::from_slice::<HashMap<String, Entry>>(&b).unwrap_or_else(|_| {
                crate::config::quarantine(&path); // keep the unreadable file instead of overwriting it
                HashMap::new()
            }),
            Err(_) => HashMap::new(),
        };
        Self {
            map,
            path: Some(path),
            dirty: false,
        }
    }

    pub fn in_memory() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// Remember that `id` was launched while the query box contained `query`.
    pub fn record(&mut self, id: &str, query: &str, now: u64) {
        self.record_with_path(id, query, now, None);
    }

    pub fn record_with_path(&mut self, id: &str, query: &str, now: u64, path: Option<&str>) {
        let e = self.map.entry(id.to_string()).or_default();
        e.s = Self::eff(e, now) + 1.0;
        e.count = e.count.saturating_add(1);
        e.last = now;
        if let Some(p) = path {
            e.p = Some(p.to_string());
        }
        let nq = normalize_query(query);
        e.sk.remove(&nq);
        if !nq.is_empty() {
            *e.q.entry(nq).or_insert(0) += 1;
            if e.q.len() > MAX_QUERIES_PER_ENTRY {
                if let Some(min_key) = e.q.iter().min_by_key(|(_, c)| **c).map(|(k, _)| k.clone()) {
                    e.q.remove(&min_key);
                }
            }
        }
        self.dirty = true;
        if self.map.len() > MAX_ENTRIES {
            self.prune(now);
        }
    }

    /// `id` was listed above the item the user actually picked for `query`: nudge it down a bit.
    pub fn record_skip(&mut self, id: &str, query: &str, now: u64) {
        let nq = normalize_query(query);
        if nq.is_empty() {
            return;
        }
        let e = self.map.entry(id.to_string()).or_default();
        if e.count == 0 {
            e.last = now;
        }
        // Competitive learning: the user chose something else for this text, so this item loses
        // one vote for it. Without this, an old favourite would keep winning for many picks.
        if let Some(c) = e.q.get_mut(&nq) {
            *c = c.saturating_sub(1);
            if *c == 0 {
                e.q.remove(&nq);
            }
        }
        *e.sk.entry(nq).or_insert(0) += 1;
        if e.sk.len() > MAX_QUERIES_PER_ENTRY {
            if let Some(k) = e.sk.iter().min_by_key(|(_, c)| **c).map(|(k, _)| k.clone()) {
                e.sk.remove(&k);
            }
        }
        self.dirty = true;
    }

    /// Learned files/folders: (id, original path).
    pub fn paths(&self) -> impl Iterator<Item = (&str, &str)> {
        self.map
            .iter()
            .filter(|(_, e)| e.count > 0)
            .filter_map(|(k, e)| e.p.as_deref().map(|p| (k.as_str(), p)))
    }

    pub fn forget(&mut self, id: &str) {
        if self.map.remove(id).is_some() {
            self.dirty = true;
        }
    }

    /// Pure exponential fade (no floor): old usage really does stop counting.
    fn decay(last: u64, now: u64) -> f32 {
        let days = now.saturating_sub(last) as f64 / 86_400.0;
        0.5f64.powf(days / HALF_LIFE_DAYS) as f32
    }

    /// Learned query associations fade much more slowly: "I pick X for this text" is a habit.
    fn assoc_decay(last: u64, now: u64) -> f32 {
        Self::decay(last, now).max(0.5)
    }

    /// Effective (decayed) launch score right now.
    fn eff(e: &Entry, now: u64) -> f32 {
        let base = if e.s > 0.0 { e.s } else { e.count as f32 };
        base * Self::decay(e.last, now)
    }

    /// Query-independent boost, capped so it can only break near-ties, never override relevance.
    fn frequency(e: &Entry, now: u64) -> f32 {
        (14.0 * (1.0 + Self::eff(e, now)).ln()).min(FREQ_CAP)
    }

    /// Score boost for `id` given the normalized current query (may be empty).
    ///
    /// `quality` (0..=1) is how well the item matches the query as text. The frequency part is
    /// scaled by it, so a heavily used item that only weakly matches does not leapfrog items that
    /// match much better. The association part ("you picked this for exactly this text") is not
    /// scaled: it is direct evidence of intent.
    pub fn boost(&self, id: &str, query_norm: &str, now: u64, quality: f32) -> f32 {
        let Some(e) = self.map.get(id) else {
            return 0.0;
        };
        let mut b = Self::frequency(e, now) * (0.4 + 0.6 * quality.clamp(0.0, 1.0));
        if !query_norm.is_empty() {
            let mut assoc = 0.0f32;
            for (sq, c) in &e.q {
                let w = if sq == query_norm {
                    50.0
                } else if sq.starts_with(query_norm) {
                    36.0
                } else if query_norm.starts_with(sq.as_str()) {
                    18.0
                } else {
                    continue;
                };
                // 1 pick = w, then diminishing returns.
                assoc = assoc.max(w * (1.0 + 0.8 * (*c as f32).ln()));
            }
            b += assoc.min(ASSOC_CAP) * Self::assoc_decay(e.last, now);
            if let Some(n) = e.sk.get(query_norm) {
                b -= (5.0 * ((1 + *n) as f32).ln()).min(15.0);
            }
        }
        b
    }

    /// Most useful items for the empty-query view.
    pub fn top(&self, n: usize, now: u64) -> Vec<(&str, f32)> {
        let mut v: Vec<(&str, f32)> = self
            .map
            .iter()
            .filter(|(_, e)| e.count > 0)
            .map(|(k, e)| (k.as_str(), Self::eff(e, now)))
            .collect();
        v.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        v.truncate(n);
        v
    }

    fn prune(&mut self, now: u64) {
        let mut scored: Vec<(String, f32)> = self
            .map
            .iter()
            .map(|(k, e)| (k.clone(), Self::eff(e, now)))
            .collect();
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        for (k, _) in scored.into_iter().skip(MAX_ENTRIES * 3 / 4) {
            self.map.remove(&k);
        }
    }

    pub fn save_if_dirty(&mut self) {
        if !self.dirty {
            return;
        }
        let Some(path) = &self.path else { return };
        if let Ok(bytes) = serde_json::to_vec(&self.map) {
            if crate::config::write_atomic(path, &bytes) {
                self.dirty = false;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: u64 = 86_400;

    #[test]
    fn picked_item_rises_for_same_query() {
        let mut l = Learner::in_memory();
        let now = 100 * DAY;
        assert_eq!(l.boost("a", "vs", now, 1.0), 0.0);
        l.record("a", "vs", now);
        let b1 = l.boost("a", "vs", now, 1.0);
        l.record("a", "vs", now);
        let b2 = l.boost("a", "vs", now, 1.0);
        assert!(b1 > 30.0, "one pick should already be a strong nudge: {b1}");
        assert!(b2 > b1);
    }

    #[test]
    fn prefix_queries_inherit_boost() {
        let mut l = Learner::in_memory();
        let now = 10 * DAY;
        l.record("a", "visual", now);
        assert!(l.boost("a", "vis", now, 1.0) > 20.0);
        assert!(l.boost("a", "visual studio", now, 1.0) > 10.0);
        assert!(l.boost("a", "zzz", now, 1.0) < l.boost("a", "visual", now, 1.0));
    }

    #[test]
    fn decays_over_time_but_not_to_zero() {
        let mut l = Learner::in_memory();
        l.record("a", "x", 0);
        let fresh = l.boost("a", "", 0, 1.0);
        let old = l.boost("a", "", 400 * DAY, 1.0);
        assert!(old < fresh * 0.01 && old >= 0.0, "usage fades out: {old} vs {fresh}");
    }

    #[test]
    fn forget_and_top() {
        let mut l = Learner::in_memory();
        l.record("a", "", 0);
        l.record("a", "", 0);
        l.record("b", "", 0);
        assert_eq!(l.top(2, 0)[0].0, "a");
        l.forget("a");
        assert_eq!(l.boost("a", "", 0, 1.0), 0.0);
    }

    #[test]
    fn skipped_items_sink_for_that_query_only() {
        let mut l = Learner::in_memory();
        l.record("pick", "vs", 0);
        let before = l.boost("skipme", "vs", 0, 1.0);
        l.record_skip("skipme", "vs", 0);
        l.record_skip("skipme", "vs", 0);
        assert!(l.boost("skipme", "vs", 0, 1.0) < before);
        assert_eq!(l.boost("skipme", "other", 0, 1.0), 0.0);
        // Skip-only entries never show up as "frequent" items.
        assert!(l.top(5, 0).iter().all(|(k, _)| *k != "skipme"));
        // Picking it later clears the penalty for that query.
        l.record("skipme", "vs", 0);
        assert!(l.boost("skipme", "vs", 0, 1.0) > 30.0);
    }

    #[test]
    fn heavy_use_cannot_override_a_much_better_text_match() {
        let mut l = Learner::in_memory();
        let now = 50 * DAY;
        for _ in 0..200 {
            l.record("heavy", "x", now);
        }
        // Unrelated query, weak text match: bounded and scaled down.
        let weak = l.boost("heavy", "zzz", now, 0.2);
        let strong = l.boost("heavy", "zzz", now, 1.0);
        assert!(weak < 25.0, "{weak}");
        assert!(strong <= FREQ_CAP + 0.01 && weak < strong);
    }

    #[test]
    fn new_favourite_replaces_old_one_for_the_same_query() {
        let mut l = Learner::in_memory();
        for _ in 0..3 {
            l.record("old", "vs", 0);
        }
        // From now on the user picks "new"; "old" is listed above it each time.
        for _ in 0..3 {
            l.record("new", "vs", 0);
            l.record_skip("old", "vs", 0);
        }
        assert!(l.boost("new", "vs", 0, 1.0) > l.boost("old", "vs", 0, 1.0));
    }

    #[test]
    fn recent_use_beats_stale_heavy_use() {
        let mut l = Learner::in_memory();
        for _ in 0..50 {
            l.record("stale", "", 0);
        }
        let now = 300 * DAY;
        for _ in 0..3 {
            l.record("fresh", "", now);
        }
        assert_eq!(l.top(2, now)[0].0, "fresh");
    }

    #[test]
    fn legacy_entries_without_score_still_work() {
        let json = r#"{"a":{"count":5,"last":0,"q":{"vs":5}}}"#;
        let map: HashMap<String, Entry> = serde_json::from_str(json).unwrap();
        let l = Learner { map, path: None, dirty: false };
        assert!(l.boost("a", "vs", 0, 1.0) > 50.0);
        assert_eq!(l.top(1, 0)[0].0, "a");
    }

    #[test]
    fn paths_are_remembered() {
        let mut l = Learner::in_memory();
        l.record_with_path(r"c:\docs\notes.txt", "n", 0, Some(r"C:\Docs\Notes.txt"));
        l.record("plain", "x", 0);
        let v: Vec<_> = l.paths().collect();
        assert_eq!(v, vec![(r"c:\docs\notes.txt", r"C:\Docs\Notes.txt")]);
    }

    #[test]
    fn query_cap_enforced() {
        let mut l = Learner::in_memory();
        for i in 0..20 {
            l.record("a", &format!("q{i}"), 0);
        }
        assert!(l.map["a"].q.len() <= MAX_QUERIES_PER_ENTRY);
    }
}
