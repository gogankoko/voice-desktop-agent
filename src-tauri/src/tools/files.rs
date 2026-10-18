//! Finding the person's files by NAME in a few permitted folders (Downloads, Pictures,
//! Documents, Desktop). Off unless the person turns it on in Settings.
//!
//! Never reads file contents, never changes, moves or deletes anything. Full paths stay in
//! Rust: the model only sees the file name, a friendly folder label, date and size.
//! "Open" is limited to everyday file types; programs, scripts, shortcuts and web pages are
//! never opened (only shown in their folder).

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use windows::Win32::Foundation::{FILETIME, SYSTEMTIME};
use windows::Win32::Storage::FileSystem::FileTimeToLocalFileTime;
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::System::Time::FileTimeToSystemTime;
use windows::Win32::UI::Shell::{
    FOLDERID_Desktop, FOLDERID_Documents, FOLDERID_Downloads, FOLDERID_Pictures, KF_FLAG_DEFAULT,
    SHGetKnownFolderPath, ShellExecuteW,
};
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use windows::core::{GUID, HSTRING, w};

const MAX_DEPTH: usize = 4;
const MAX_SCANNED: usize = 20_000;
pub const MAX_RESULTS: usize = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileKind {
    Photo,
    Document,
    Video,
    Music,
    Any,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum When {
    Today,
    Yesterday,
    ThisWeek,
    Any,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FoundFile {
    /// Kept in Rust; never sent to the model.
    pub path: PathBuf,
    pub name: String,
    /// e.g. "Downloads" or "Pictures › Holiday".
    pub folder: String,
    /// e.g. "today at 14:05", "yesterday at 09:12", "3 March 2026".
    pub modified: String,
    pub size: String,
}

const PHOTO: &[&str] = &[
    "jpg", "jpeg", "png", "gif", "heic", "heif", "webp", "bmp", "tif", "tiff",
];
const DOCUMENT: &[&str] = &[
    "pdf", "doc", "docx", "txt", "rtf", "odt", "xls", "xlsx", "csv", "ppt", "pptx", "odp", "ods",
];
const VIDEO: &[&str] = &["mp4", "mov", "avi", "mkv", "wmv", "m4v", "webm"];
const MUSIC: &[&str] = &["mp3", "wav", "m4a", "flac", "aac", "wma", "ogg"];

fn ext_of(p: &Path) -> String {
    p.extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

pub fn kind_of(p: &Path) -> Option<FileKind> {
    let e = ext_of(p);
    let e = e.as_str();
    if PHOTO.contains(&e) {
        Some(FileKind::Photo)
    } else if DOCUMENT.contains(&e) {
        Some(FileKind::Document)
    } else if VIDEO.contains(&e) {
        Some(FileKind::Video)
    } else if MUSIC.contains(&e) {
        Some(FileKind::Music)
    } else {
        None
    }
}

/// Only everyday file types may be opened. Everything else (programs, scripts, shortcuts,
/// installers, web pages, unknown types) can only be shown in its folder.
pub fn safe_to_open(p: &Path) -> bool {
    kind_of(p).is_some()
}

fn known_folder(id: &GUID) -> Option<PathBuf> {
    let p = unsafe { SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None) }.ok()?;
    let s = unsafe { p.to_string() }.ok();
    unsafe { CoTaskMemFree(Some(p.0 as *const _)) };
    s.map(PathBuf::from)
}

/// The folders the assistant may search, with their plain names.
pub fn permitted_folders() -> Vec<(&'static str, PathBuf)> {
    [
        ("Downloads", &FOLDERID_Downloads),
        ("Pictures", &FOLDERID_Pictures),
        ("Documents", &FOLDERID_Documents),
        ("Desktop", &FOLDERID_Desktop),
    ]
    .into_iter()
    .filter_map(|(name, id)| known_folder(id).map(|p| (name, p)))
    .collect()
}

/// True if `p` is inside one of the permitted folders (after resolving `..` and links).
pub fn is_permitted(p: &Path) -> bool {
    let Ok(real) = p.canonicalize() else {
        return false;
    };
    permitted_folders()
        .iter()
        .filter_map(|(_, root)| root.canonicalize().ok())
        .any(|root| real.starts_with(root))
}

fn local_date(t: SystemTime) -> Option<SYSTEMTIME> {
    // FILETIME counts 100 ns intervals since 1601-01-01 (UTC).
    let since_unix = t.duration_since(UNIX_EPOCH).ok()?;
    let ticks = since_unix.as_nanos() / 100 + 116_444_736_000_000_000;
    let ft = FILETIME {
        dwLowDateTime: ticks as u32,
        dwHighDateTime: (ticks >> 32) as u32,
    };
    let mut local = FILETIME::default();
    let mut st = SYSTEMTIME::default();
    unsafe {
        FileTimeToLocalFileTime(&ft, &mut local).ok()?;
        FileTimeToSystemTime(&local, &mut st).ok()?;
    }
    Some(st)
}

fn same_day(a: &SYSTEMTIME, b: &SYSTEMTIME) -> bool {
    (a.wYear, a.wMonth, a.wDay) == (b.wYear, b.wMonth, b.wDay)
}

/// Does a file modified at `modified` match `when`, as seen at `now`?
pub fn matches_when(when: When, modified: SystemTime, now: SystemTime) -> bool {
    match when {
        When::Any => true,
        When::ThisWeek => now
            .duration_since(modified)
            .map(|d| d <= Duration::from_secs(7 * 24 * 3600))
            .unwrap_or(true),
        When::Today | When::Yesterday => {
            let day = if when == When::Today {
                now
            } else {
                now - Duration::from_secs(24 * 3600)
            };
            match (local_date(modified), local_date(day)) {
                (Some(m), Some(d)) => same_day(&m, &d),
                _ => false,
            }
        }
    }
}

fn modified_label(modified: SystemTime, now: SystemTime) -> String {
    const MONTHS: [&str; 12] = [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ];
    let Some(m) = local_date(modified) else {
        return String::new();
    };
    let time = format!("{:02}:{:02}", m.wHour, m.wMinute);
    if matches_when(When::Today, modified, now) {
        format!("today at {time}")
    } else if matches_when(When::Yesterday, modified, now) {
        format!("yesterday at {time}")
    } else {
        format!(
            "{} {} {}",
            m.wDay,
            MONTHS.get(m.wMonth as usize - 1).unwrap_or(&""),
            m.wYear
        )
    }
}

fn size_label(bytes: u64) -> String {
    match bytes {
        b if b >= 1 << 30 => format!("{:.1} GB", b as f64 / (1u64 << 30) as f64),
        b if b >= 1 << 20 => format!("{:.1} MB", b as f64 / (1u64 << 20) as f64),
        b if b >= 1 << 10 => format!("{} KB", b / 1024),
        b => format!("{b} bytes"),
    }
}

fn skip_dir(name: &str) -> bool {
    name.starts_with('.') || name.starts_with('$') || matches!(name, "node_modules" | "AppData")
}

/// Searches the permitted folders. Newest first, at most `MAX_RESULTS`.
pub fn find_files(kind: FileKind, when: When, name_contains: Option<&str>) -> Vec<FoundFile> {
    let needle = name_contains
        .map(|n| n.trim().to_lowercase())
        .filter(|n| !n.is_empty());
    let now = SystemTime::now();
    let mut hits: Vec<(SystemTime, FoundFile)> = Vec::new();
    let mut scanned = 0usize;
    for (label, root) in permitted_folders() {
        let mut stack = vec![(root.clone(), 0usize, label.to_string())];
        while let Some((dir, depth, folder)) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                scanned += 1;
                if scanned > MAX_SCANNED {
                    break;
                }
                let name = entry.file_name().to_string_lossy().to_string();
                let Ok(meta) = entry.metadata() else { continue };
                if meta.is_dir() {
                    if depth < MAX_DEPTH && !skip_dir(&name) {
                        stack.push((entry.path(), depth + 1, format!("{folder} \u{203a} {name}")));
                    }
                    continue;
                }
                let path = entry.path();
                let file_kind = kind_of(&path);
                if kind != FileKind::Any && file_kind != Some(kind) {
                    continue;
                }
                if let Some(n) = &needle {
                    if !name.to_lowercase().contains(n.as_str()) {
                        continue;
                    }
                }
                let Ok(modified) = meta.modified() else {
                    continue;
                };
                if !matches_when(when, modified, now) {
                    continue;
                }
                hits.push((
                    modified,
                    FoundFile {
                        path,
                        name,
                        folder: folder.clone(),
                        modified: modified_label(modified, now),
                        size: size_label(meta.len()),
                    },
                ));
            }
        }
    }
    hits.sort_by(|a, b| b.0.cmp(&a.0));
    hits.into_iter().take(MAX_RESULTS).map(|(_, f)| f).collect()
}

/// Opens File Explorer with the file selected. Works for any file type (nothing runs).
pub fn show_in_folder(p: &Path) -> Result<(), String> {
    if !p.is_file() || !is_permitted(p) {
        return Err("That file isn't there any more.".into());
    }
    Command::new("explorer.exe")
        .arg(format!("/select,{}", p.display()))
        .spawn()
        .map(|_| ())
        .map_err(|_| "I couldn't open File Explorer.".to_string())
}

/// Opens an everyday file (photo, document, video, music) with its usual app.
pub fn open_file(p: &Path) -> Result<(), String> {
    if !p.is_file() || !is_permitted(p) {
        return Err("That file isn't there any more.".into());
    }
    if !safe_to_open(p) {
        return Err("For your safety I only open photos, documents, videos and music. I can show this file in its folder instead.".into());
    }
    let r = unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            &HSTRING::from(p.as_os_str()),
            None,
            None,
            SW_SHOWNORMAL,
        )
    };
    if (r.0 as isize) <= 32 {
        return Err("Windows couldn't open that file.".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_and_safe_types() {
        assert_eq!(kind_of(Path::new("Holiday.JPG")), Some(FileKind::Photo));
        assert_eq!(kind_of(Path::new("letter.docx")), Some(FileKind::Document));
        for dangerous in [
            "setup.exe",
            "run.bat",
            "x.cmd",
            "s.ps1",
            "a.js",
            "a.vbs",
            "link.lnk",
            "i.msi",
            "s.scr",
            "page.html",
            "page.htm",
            "noext",
            "archive.zip",
        ] {
            assert!(!safe_to_open(Path::new(dangerous)), "{dangerous}");
        }
        for ok in ["a.pdf", "b.png", "c.mp4", "d.mp3", "e.txt"] {
            assert!(safe_to_open(Path::new(ok)), "{ok}");
        }
    }

    #[test]
    fn date_filters() {
        let now = SystemTime::now();
        let hour = Duration::from_secs(3600);
        assert!(matches_when(When::Any, now - hour * 24 * 400, now));
        assert!(matches_when(When::ThisWeek, now - hour * 24 * 3, now));
        assert!(!matches_when(When::ThisWeek, now - hour * 24 * 9, now));
        assert!(matches_when(When::Today, now, now));
        assert!(matches_when(When::Yesterday, now - hour * 24, now));
        assert!(!matches_when(When::Yesterday, now, now));
    }

    #[test]
    fn labels() {
        let now = SystemTime::now();
        assert!(modified_label(now, now).starts_with("today at "));
        assert!(
            modified_label(now - Duration::from_secs(24 * 3600), now).starts_with("yesterday at ")
        );
        assert_eq!(size_label(500), "500 bytes");
        assert_eq!(size_label(2 * 1024 * 1024), "2.0 MB");
    }

    #[test]
    fn permitted_folders_resolve_and_outside_paths_are_refused() {
        let folders = permitted_folders();
        assert!(folders.iter().any(|(n, _)| *n == "Downloads"));
        assert!(!is_permitted(Path::new(r"C:\Windows\System32\notepad.exe")));
        assert!(open_file(Path::new(r"C:\Windows\System32\notepad.exe")).is_err());
        assert!(show_in_folder(Path::new(r"C:\Windows\win.ini")).is_err());
    }

    #[test]
    fn search_is_bounded() {
        assert!(find_files(FileKind::Any, When::Any, None).len() <= MAX_RESULTS);
    }

    /// Live, privacy-preserving: searches the real folders but prints only counts, never names.
    /// cargo test --manifest-path src-tauri/Cargo.toml live_files -- --ignored --nocapture
    #[test]
    #[ignore = "scans the owner's real folders (prints counts only)"]
    fn live_files_counts_only() {
        let folders = permitted_folders();
        println!(
            "permitted folders found: {:?}",
            folders.iter().map(|(n, _)| *n).collect::<Vec<_>>()
        );
        for (kind, when) in [
            (FileKind::Photo, When::Any),
            (FileKind::Document, When::Any),
            (FileKind::Any, When::ThisWeek),
        ] {
            let started = std::time::Instant::now();
            let found = find_files(kind, when, None);
            println!(
                "{kind:?}/{when:?}: {} result(s) in {} ms; sample date label: {:?}",
                found.len(),
                started.elapsed().as_millis(),
                found.first().map(|f| f.modified.clone())
            );
            assert!(found.len() <= MAX_RESULTS);
            assert!(found.iter().all(|f| is_permitted(&f.path)));
        }
    }
}
