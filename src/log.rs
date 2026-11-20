//! Tiny append-only log (`data\swiftfind.log`) plus a panic hook.
//!
//! The release build has no console, so without this a failure would be invisible. The log is
//! rotated at 256 KB (one `.old` copy is kept), so it can never grow without bound.

use std::io::Write;
use std::path::PathBuf;

const MAX_BYTES: u64 = 256 * 1024;

pub fn log_path() -> PathBuf {
    crate::config::data_dir().join("swiftfind.log")
}

/// `YYYY-MM-DD HH:MM:SS` (UTC) from Unix seconds. Civil-from-days algorithm (H. Hinnant).
pub fn format_utc(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02}",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

pub fn write_to(path: &std::path::Path, msg: &str) {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if std::fs::metadata(path)
        .map(|m| m.len() > MAX_BYTES)
        .unwrap_or(false)
    {
        let _ = std::fs::rename(path, path.with_extension("log.old"));
    }
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = writeln!(
            f,
            "[{} UTC] {}",
            format_utc(secs),
            msg.replace('\n', "\n    ")
        );
    }
}

pub fn info(msg: &str) {
    write_to(&log_path(), msg);
}

/// Log panics (from any thread) instead of letting them vanish; the main thread additionally
/// shows a message box. Worker threads are wrapped so a panic there does not end the program.
pub fn install_panic_hook() {
    std::panic::set_hook(Box::new(|p| {
        let thread = std::thread::current();
        let name = thread.name().unwrap_or("?");
        let payload = p
            .payload()
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| p.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "unknown panic".into());
        let loc = p
            .location()
            .map(|l| format!("{}:{}", l.file(), l.line()))
            .unwrap_or_default();
        info(&format!("PANIC in thread '{name}': {payload} ({loc})"));
        if name == "main" {
            crate::platform::error_box(&format!(
                "SwiftFind 发生内部错误并将退出。\n详情见 {}",
                log_path().display()
            ));
        }
    }));
}

/// Run `f`, logging (instead of propagating) a panic. Returns whether it completed normally.
pub fn guarded(what: &str, f: impl FnOnce()) -> bool {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(()) => true,
        Err(_) => {
            info(&format!("recovered from panic in {what}"));
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utc_formatting() {
        assert_eq!(format_utc(0), "1970-01-01 00:00:00");
        assert_eq!(format_utc(951_782_400), "2000-02-29 00:00:00"); // leap day
        assert_eq!(format_utc(1795136461), "2026-11-20 01:01:01");
    }

    #[test]
    fn rotates_when_large() {
        let dir = std::env::temp_dir().join(format!("sf_log_{}", std::process::id()));
        let p = dir.join("t.log");
        write_to(&p, &"x".repeat(MAX_BYTES as usize + 10));
        write_to(&p, "fresh\nsecond line");
        let cur = std::fs::read_to_string(&p).unwrap();
        assert!(cur.contains("fresh") && cur.contains("\n    second line") && cur.len() < 200);
        assert!(p.with_extension("log.old").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn guarded_recovers() {
        let _ = std::panic::take_hook(); // keep test output quiet
        assert!(guarded("ok", || {}));
        assert!(!guarded("boom", || panic!("expected in test")));
    }
}
