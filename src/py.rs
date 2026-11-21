//! Pinyin keywords for Chinese names, so "wx" / "weixin" finds 微信.
//!
//! Uses the pure-Rust `pinyin` crate. For each name we produce the full pinyin and the initials,
//! plus variants for polyphonic characters (音乐 is "yinyue", not "yinle"), as space-separated
//! lowercase tokens that the matcher treats like any other keyword. Names without Chinese
//! characters produce nothing, so they cost no memory.

use pinyin::ToPinyinMulti;

/// At most this many pronunciation variants per name (each differs in one polyphonic character).
const MAX_VARIANTS: usize = 8;

enum Seg {
    /// Readings of one Chinese character, most common first.
    Han(Vec<&'static str>),
    /// A run of ASCII letters/digits, lowercased.
    Ascii(String),
}

fn segments(name: &str) -> Vec<Seg> {
    let mut out: Vec<Seg> = Vec::new();
    let mut run = String::new();
    for c in name.chars() {
        if let Some(multi) = c.to_pinyin_multi() {
            if !run.is_empty() {
                out.push(Seg::Ascii(std::mem::take(&mut run)));
            }
            let readings: Vec<&'static str> = multi.into_iter().map(|p| p.plain()).collect();
            out.push(Seg::Han(readings));
        } else if c.is_ascii_alphanumeric() {
            run.push(c.to_ascii_lowercase());
        } else if !run.is_empty() {
            out.push(Seg::Ascii(std::mem::take(&mut run)));
        }
    }
    if !run.is_empty() {
        out.push(Seg::Ascii(run));
    }
    out
}

/// Space-separated pinyin tokens for `name`, or an empty string if it has no Chinese.
pub fn keywords(name: &str) -> String {
    let segs = segments(name);
    if !segs.iter().any(|s| matches!(s, Seg::Han(_))) {
        return String::new();
    }
    // Variant 0 uses the first reading everywhere; others swap in one alternative reading.
    let mut choices: Vec<Vec<usize>> = vec![vec![0; segs.len()]];
    'outer: for (i, s) in segs.iter().enumerate() {
        if let Seg::Han(r) = s {
            for alt in 1..r.len() {
                if choices.len() >= MAX_VARIANTS {
                    break 'outer;
                }
                let mut v = vec![0; segs.len()];
                v[i] = alt;
                choices.push(v);
            }
        }
    }
    let mut tokens: Vec<String> = Vec::new();
    for pick in &choices {
        let (mut full, mut initials) = (String::new(), String::new());
        for (s, &k) in segs.iter().zip(pick) {
            match s {
                Seg::Han(r) => {
                    let p = r[k];
                    full.push_str(p);
                    initials.extend(p.chars().next());
                }
                Seg::Ascii(a) => {
                    full.push_str(a);
                    initials.push_str(a);
                }
            }
        }
        for t in [full, initials] {
            if !tokens.contains(&t) {
                tokens.push(t);
            }
        }
    }
    tokens.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn has(name: &str, tok: &str) -> bool {
        keywords(name).split(' ').any(|t| t == tok)
    }

    #[test]
    fn full_pinyin_and_initials() {
        assert!(has("微信", "weixin") && has("微信", "wx"));
        assert!(has("腾讯会议", "tengxunhuiyi") && has("腾讯会议", "txhy"));
    }

    #[test]
    fn polyphonic_characters_get_variants() {
        // 乐 reads "le" first but "yue" in 音乐; 网易云音乐 must be findable as wangyiyunyinyue.
        assert!(has("网易云音乐", "wangyiyunyinyue"), "{}", keywords("网易云音乐"));
        assert!(has("网易云音乐", "wyyyy"));
        assert!(has("银行", "yinhang") && has("银行", "yinxing"));
    }

    #[test]
    fn mixed_ascii_and_chinese() {
        assert!(has("QQ音乐", "qqyinyue") || has("QQ音乐", "qqyinle"));
        assert!(has("QQ音乐", "qqyy"));
        assert!(has("Office 办公", "officebangong") && has("Office 办公", "officebg"));
    }

    #[test]
    fn plain_names_cost_nothing() {
        assert_eq!(keywords("Visual Studio Code"), "");
        assert_eq!(keywords(""), "");
    }

    #[test]
    fn variants_are_capped() {
        let k = keywords("行行行行行行行行行行行行");
        assert!(k.split(' ').count() <= MAX_VARIANTS * 2);
    }
}
