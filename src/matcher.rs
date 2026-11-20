//! Fast in-memory fuzzy matcher for local sources (apps, settings, commands).
//! Files are matched by Everything itself; this module only scores local items.

/// Pre-lowercased searchable text, built once per item so each keystroke
/// does no allocation on the hot path.
#[derive(Clone, Debug, Default)]
pub struct Haystack {
    pub title: String,
    pub keywords: String,
    pub initials: String,
}

impl Haystack {
    pub fn new(title: &str, keywords: &str) -> Self {
        let title_lc = title.to_lowercase();
        let initials = initials_of(&title_lc);
        Self {
            title: title_lc,
            keywords: keywords.to_lowercase(),
            initials,
        }
    }
}

fn initials_of(s: &str) -> String {
    let mut out = String::new();
    let mut at_start = true;
    for c in s.chars() {
        if c.is_alphanumeric() {
            if at_start {
                out.push(c);
            }
            at_start = false;
        } else {
            at_start = true;
        }
    }
    out
}

fn is_boundary(prev: Option<char>) -> bool {
    match prev {
        None => true,
        Some(c) => !c.is_alphanumeric(),
    }
}

/// Position-aware substring search that reports whether the hit sits on a word boundary.
fn find_with_boundary(hay: &str, needle: &str) -> Option<(usize, bool)> {
    let mut best: Option<(usize, bool)> = None;
    let mut from = 0;
    while let Some(rel) = hay[from..].find(needle) {
        let pos = from + rel;
        let prev = hay[..pos].chars().next_back();
        let boundary = is_boundary(prev);
        match best {
            None => best = Some((pos, boundary)),
            Some((_, false)) if boundary => best = Some((pos, true)),
            _ => {}
        }
        if boundary {
            break;
        }
        from = pos + needle.chars().next().map(|c| c.len_utf8()).unwrap_or(1);
        if from >= hay.len() {
            break;
        }
    }
    best
}

/// Subsequence match with a bonus for consecutive characters. Returns 0.0 if no match.
fn subsequence_score(hay: &str, needle: &str) -> f32 {
    let mut chars = hay.chars();
    let mut run = 0u32;
    let mut best_run = 0u32;
    let mut matched = 0u32;
    let total = needle.chars().count() as u32;
    for n in needle.chars() {
        let mut found = false;
        for h in chars.by_ref() {
            if h == n {
                found = true;
                break;
            } else {
                run = 0;
            }
        }
        if !found {
            return 0.0;
        }
        matched += 1;
        run += 1;
        best_run = best_run.max(run);
    }
    if matched < total || total < 2 {
        return 0.0;
    }
    18.0 + (best_run as f32 / total as f32) * 14.0
}

fn token_score(h: &Haystack, tok: &str) -> f32 {
    let mut best = 0.0f32;
    let title = h.title.as_str();
    if title == tok {
        return 100.0;
    }
    if title.starts_with(tok) {
        best = best.max(85.0);
    } else if let Some((_, boundary)) = find_with_boundary(title, tok) {
        best = best.max(if boundary { 70.0 } else { 50.0 });
    }
    if !h.keywords.is_empty() {
        for kw in h.keywords.split_whitespace() {
            if kw == tok {
                best = best.max(78.0);
            } else if kw.starts_with(tok) {
                best = best.max(62.0);
            } else if tok.chars().count() >= 2 && kw.contains(tok) {
                best = best.max(36.0);
            }
        }
        // Multi-word keyword phrases (stored with '|' separators).
        if h.keywords.contains('|') {
            for phrase in h.keywords.split('|') {
                let p = phrase.trim();
                if p == tok {
                    best = best.max(78.0);
                } else if p.contains(tok) && tok.chars().count() >= 2 {
                    best = best.max(40.0);
                }
            }
        }
    }
    if best < 55.0 && tok.chars().count() >= 2 && h.initials.starts_with(tok) {
        best = best.max(58.0);
    }
    if best == 0.0 {
        best = subsequence_score(title, tok);
    }
    best
}

/// Score a haystack against whitespace-separated query tokens (already lowercased).
/// Every token must match. Returns `None` when the item does not match.
pub fn score(h: &Haystack, tokens: &[&str]) -> Option<f32> {
    if tokens.is_empty() {
        return None;
    }
    let mut sum = 0.0;
    for t in tokens {
        let s = token_score(h, t);
        if s <= 0.0 {
            return None;
        }
        sum += s;
    }
    let avg = sum / tokens.len() as f32;
    // Prefer shorter titles slightly: "Code" over "Code Helper (Renderer)".
    let len_penalty = (h.title.chars().count() as f32 * 0.08).min(8.0);
    Some((avg - len_penalty).max(1.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(title: &str, kw: &str, q: &str) -> Option<f32> {
        let h = Haystack::new(title, kw);
        let q = q.to_lowercase();
        let toks: Vec<&str> = q.split_whitespace().collect();
        score(&h, &toks)
    }

    #[test]
    fn exact_beats_prefix_beats_substring() {
        let exact = s("code", "", "code").unwrap();
        let prefix = s("code helper", "", "code").unwrap();
        let sub = s("barcode scanner", "", "code").unwrap();
        assert!(exact > prefix && prefix > sub, "{exact} {prefix} {sub}");
    }

    #[test]
    fn initials_match() {
        assert!(s("Visual Studio Code", "", "vsc").unwrap() > 50.0);
    }

    #[test]
    fn keyword_match_and_chinese() {
        assert!(s("环境变量", "huanjing hjbl env environment", "hjbl").is_some());
        assert!(s("环境变量", "", "环境").unwrap() > 80.0);
        assert!(s("防火墙", "firewall", "firewall").unwrap() > 70.0);
    }

    #[test]
    fn all_tokens_required() {
        assert!(s("Windows Defender Firewall", "", "fire win").is_some());
        assert!(s("Windows Defender Firewall", "", "fire zzz").is_none());
    }

    #[test]
    fn fuzzy_subsequence() {
        assert!(s("Task Manager", "", "tskmgr").is_some());
        assert!(s("Task Manager", "", "xyz").is_none());
    }
}
