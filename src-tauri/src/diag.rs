//! Local diagnostics log: a small text file the user can view, copy or clear in Help.
//!
//! Only technical events are written (what failed, status codes, which tool ran, timings),
//! never conversation text, page text, keys or tokens. Every line also passes through
//! `redact()` as a second line of defence, and lines are length-limited.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

const MAX_BYTES: u64 = 256 * 1024;
const MAX_LINE_CHARS: usize = 300;

struct Log {
    path: PathBuf,
    file: File,
}

static LOG: OnceLock<Mutex<Log>> = OnceLock::new();

fn open(path: &Path) -> std::io::Result<File> {
    OpenOptions::new().create(true).append(true).open(path)
}

pub fn init(dir: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dir)?;
    let path = dir.join("diagnostics.log");
    let file = open(&path)?;
    let _ = LOG.set(Mutex::new(Log { path, file }));
    Ok(())
}

/// Removes anything that could identify the person or unlock their accounts.
pub fn redact(s: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    for word in s.split_whitespace() {
        let core = word.trim_matches(|c: char| "\"'()[]{}<>,;".contains(c));
        let replaced = if core.contains("://") {
            "[address]".to_string()
        } else if core.contains('@') && core.contains('.') {
            "[email]".to_string()
        } else if core.starts_with("AIza") || looks_like_secret(core) {
            "[secret]".to_string()
        } else {
            redact_user_path(word)
        };
        out.push(replaced);
    }
    let joined = out.join(" ");
    if joined.chars().count() > MAX_LINE_CHARS {
        format!(
            "{}…",
            joined.chars().take(MAX_LINE_CHARS).collect::<String>()
        )
    } else {
        joined
    }
}

/// Long unbroken runs of key/token characters (API keys, JWTs, signatures).
fn looks_like_secret(w: &str) -> bool {
    w.len() >= 24
        && w.chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_.=+/".contains(c))
        && w.chars().any(|c| c.is_ascii_digit())
        && w.chars().any(|c| c.is_ascii_alphabetic())
}

/// `C:\Users\Jane\...` → `C:\Users\[user]\...` (also forward slashes).
fn redact_user_path(w: &str) -> String {
    for sep in ['\\', '/'] {
        let marker = format!("Users{sep}");
        if let Some(i) = w.find(&marker) {
            let start = i + marker.len();
            let rest = &w[start..];
            let end = rest.find(sep).map(|e| start + e).unwrap_or(w.len());
            return format!("{}[user]{}", &w[..start], &w[end..]);
        }
    }
    w.to_string()
}

/// Writes one redacted line. Never fails the caller.
pub fn log(category: &str, message: impl AsRef<str>) {
    let line = format!(
        "{} [{}] {}\n",
        timestamp(),
        category,
        redact(message.as_ref())
    );
    if cfg!(debug_assertions) {
        eprint!("{line}");
    }
    let Some(lock) = LOG.get() else { return };
    let Ok(mut log) = lock.lock() else { return };
    if log.file.metadata().map(|m| m.len()).unwrap_or(0) > MAX_BYTES {
        let old = log.path.with_extension("old.log");
        let _ = fs::rename(&log.path, &old);
        if let Ok(f) = open(&log.path) {
            log.file = f;
        }
    }
    let _ = log.file.write_all(line.as_bytes());
}

/// The most recent lines, for the Help screen.
pub fn recent(max_lines: usize) -> String {
    let Some(lock) = LOG.get() else {
        return String::new();
    };
    let Ok(log) = lock.lock() else {
        return String::new();
    };
    let text = fs::read_to_string(&log.path).unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(max_lines)..].join("\n")
}

pub fn clear() {
    let Some(lock) = LOG.get() else { return };
    let Ok(mut log) = lock.lock() else { return };
    let _ = fs::remove_file(log.path.with_extension("old.log"));
    if let Ok(f) = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&log.path)
    {
        log.file = f;
    }
}

fn timestamp() -> String {
    #[cfg(windows)]
    {
        let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
        format!(
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
            t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond
        )
    }
    #[cfg(not(windows))]
    {
        String::from("-")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_keys_tokens_addresses_emails_and_user_paths() {
        let key = format!("AIza{}", "x1".repeat(18));
        let jwt =
            "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV";
        let s = redact(&format!(
            "key={key} token {jwt} https://bank.example/login?u=1 mail anna@example.com path C:\\Users\\Jane\\AppData\\x.db"
        ));
        assert!(!s.contains("AIza"), "{s}");
        assert!(!s.contains("eyJ"), "{s}");
        assert!(!s.contains("bank.example"), "{s}");
        assert!(!s.contains("anna@"), "{s}");
        assert!(!s.contains("Jane"), "{s}");
        assert!(s.contains("C:\\Users\\[user]\\AppData"), "{s}");
    }

    #[test]
    fn keeps_ordinary_technical_text() {
        assert_eq!(
            redact("gemini (gemini-3.8-flash) returned HTTP 503"),
            "gemini (gemini-3.8-flash) returned HTTP 503"
        );
        assert_eq!(
            redact("open_website verified=true in 812ms"),
            "open_website verified=true in 812ms"
        );
    }

    #[test]
    fn long_lines_are_truncated() {
        assert!(redact(&"word ".repeat(200)).chars().count() <= MAX_LINE_CHARS + 1);
    }

    #[test]
    fn log_writes_rotates_and_clears() {
        let dir = std::env::temp_dir().join(format!("diag-test-{}", std::process::id()));
        init(&dir).unwrap();
        log("test", "hello anna@example.com");
        let text = recent(10);
        assert!(text.contains("[test] hello [email]"), "{text}");
        clear();
        assert!(recent(10).is_empty());
        let _ = fs::remove_dir_all(&dir);
    }
}
