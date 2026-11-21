//! Text snippets: a short name that pastes a longer text into the window you came from.
//!
//! Variables in the text are filled in at paste time: `{date}` `{time}` `{datetime}` `{year}`
//! `{month}` `{day}`; `{{` and `}}` give literal braces; unknown `{names}` are left untouched.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LocalTime {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
}

pub fn expand(text: &str, t: LocalTime) -> String {
    let mut out = String::with_capacity(text.len() + 16);
    let mut rest = text;
    while let Some(i) = rest.find(['{', '}']) {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        if let Some(r) = tail.strip_prefix("{{") {
            out.push('{');
            rest = r;
        } else if let Some(r) = tail.strip_prefix("}}") {
            out.push('}');
            rest = r;
        } else if tail.starts_with('{') {
            match tail.find('}') {
                Some(end) => {
                    let name = &tail[1..end];
                    match var(name, t) {
                        Some(v) => out.push_str(&v),
                        None => out.push_str(&tail[..=end]),
                    }
                    rest = &tail[end + 1..];
                }
                None => {
                    out.push_str(tail);
                    rest = "";
                }
            }
        } else {
            out.push('}'); // a lone closing brace
            rest = &tail[1..];
        }
    }
    out.push_str(rest);
    out
}

fn var(name: &str, t: LocalTime) -> Option<String> {
    Some(match name.trim().to_lowercase().as_str() {
        "date" => format!("{:04}-{:02}-{:02}", t.year, t.month, t.day),
        "time" => format!("{:02}:{:02}:{:02}", t.hour, t.minute, t.second),
        "datetime" => format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}", t.year, t.month, t.day, t.hour, t.minute, t.second),
        "year" => format!("{:04}", t.year),
        "month" => format!("{:02}", t.month),
        "day" => format!("{:02}", t.day),
        _ => return None,
    })
}

/// First line of a snippet, shortened, for the result row.
pub fn preview(text: &str) -> String {
    let line = text.lines().find(|l| !l.trim().is_empty()).unwrap_or("").trim();
    let mut s: String = line.chars().take(80).collect();
    if line.chars().count() > 80 || text.lines().filter(|l| !l.trim().is_empty()).count() > 1 {
        s.push('…');
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    const T: LocalTime = LocalTime { year: 2026, month: 3, day: 7, hour: 9, minute: 5, second: 2 };

    #[test]
    fn fills_known_variables() {
        assert_eq!(expand("今天是 {date}，{time}", T), "今天是 2026-03-07，09:05:02");
        assert_eq!(expand("{DateTime}", T), "2026-03-07 09:05:02");
        assert_eq!(expand("{year}/{month}/{day}", T), "2026/03/07");
    }

    #[test]
    fn leaves_unknown_and_handles_braces() {
        assert_eq!(expand("hi {name}!", T), "hi {name}!");
        assert_eq!(expand("{{date}} = {date}", T), "{date} = 2026-03-07");
        assert_eq!(expand("a } b", T), "a } b");
        assert_eq!(expand("unclosed {date", T), "unclosed {date");
        assert_eq!(expand("", T), "");
        assert_eq!(expand("纯文本 no braces", T), "纯文本 no braces");
    }

    #[test]
    fn preview_is_one_short_line() {
        assert_eq!(preview("first\nsecond"), "first…");
        assert_eq!(preview("only"), "only");
        assert_eq!(preview("\n\n  x  "), "x");
        assert!(preview(&"长".repeat(200)).chars().count() <= 81);
    }
}
