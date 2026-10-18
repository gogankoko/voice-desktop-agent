//! Thin, read-only wrappers over Win32 used by the tools. No input simulation here.

use std::path::PathBuf;

use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM};
use windows::Win32::Graphics::Dwm::{DWMWA_CLOAKED, DwmGetWindowAttribute};
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows::Win32::UI::Shell::{
    ASSOCF_IS_PROTOCOL, ASSOCF_NOTRUNCATE, ASSOCSTR_EXECUTABLE, AssocQueryStringW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetForegroundWindow, GetWindowTextLengthW, GetWindowTextW,
    GetWindowThreadProcessId, IsWindowVisible,
};
use windows::core::{BOOL, PWSTR, w};

#[allow(dead_code)] // title/pid are for upcoming tools (read_visible_text, focus_window).
#[derive(Debug, Clone)]
pub struct WindowInfo {
    pub title: String,
    pub pid: u32,
    pub exe_path: Option<PathBuf>,
}

/// The window currently receiving keyboard input, if any.
pub fn foreground_window() -> Option<WindowInfo> {
    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.is_invalid() {
        return None;
    }
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    Some(WindowInfo {
        title: window_title(hwnd),
        pid,
        exe_path: process_image_path(pid),
    })
}

fn window_title(hwnd: HWND) -> String {
    let len = unsafe { GetWindowTextLengthW(hwnd) };
    if len <= 0 {
        return String::new();
    }
    let mut buf = vec![0u16; len as usize + 1];
    let copied = unsafe { GetWindowTextW(hwnd, &mut buf) };
    String::from_utf16_lossy(&buf[..copied.max(0) as usize])
}

pub fn process_image_path(pid: u32) -> Option<PathBuf> {
    if pid == 0 {
        return None;
    }
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?;
    let mut buf = vec![0u16; 1024];
    let mut size = buf.len() as u32;
    let result = unsafe {
        QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_WIN32,
            PWSTR(buf.as_mut_ptr()),
            &mut size,
        )
    };
    let _ = unsafe { CloseHandle(handle) };
    result.ok()?;
    Some(PathBuf::from(String::from_utf16_lossy(
        &buf[..size as usize],
    )))
}

/// Visible, titled top-level windows in z-order (front first).
pub fn top_level_windows() -> Vec<HWND> {
    unsafe extern "system" fn collect(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let list = unsafe { &mut *(lparam.0 as *mut Vec<HWND>) };
        if unsafe { IsWindowVisible(hwnd) }.as_bool() && unsafe { GetWindowTextLengthW(hwnd) } > 0 {
            list.push(hwnd);
        }
        true.into()
    }
    let mut list: Vec<HWND> = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(collect), LPARAM(&mut list as *mut _ as isize));
    }
    list
}

/// Hidden-but-"visible" windows (suspended Store apps, background shells) are cloaked by DWM.
fn is_cloaked(hwnd: HWND) -> bool {
    let mut cloaked: u32 = 0;
    unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_CLOAKED,
            &mut cloaked as *mut u32 as *mut core::ffi::c_void,
            std::mem::size_of::<u32>() as u32,
        )
    }
    .is_ok()
        && cloaked != 0
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowSummary {
    pub title: String,
    pub app: String,
    /// Kept in Rust to read the window later; never sent to the model.
    pub hwnd: isize,
}

/// Plain name for a program, from its executable.
pub fn friendly_app_name(exe: Option<&std::path::Path>) -> String {
    let stem = exe
        .and_then(|p| p.file_stem())
        .map(|s| s.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    match stem.as_str() {
        "msedge" => "Microsoft Edge".into(),
        "chrome" => "Google Chrome".into(),
        "firefox" => "Firefox".into(),
        "explorer" => "File Explorer".into(),
        "winword" => "Word".into(),
        "excel" => "Excel".into(),
        "powerpnt" => "PowerPoint".into(),
        "outlook" | "olk" => "Outlook".into(),
        "notepad" => "Notepad".into(),
        "code" => "Visual Studio Code".into(),
        "windowsterminal" => "Terminal".into(),
        "spotify" => "Spotify".into(),
        "" => "an app".into(),
        other => {
            let mut c = other.chars();
            c.next()
                .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
                .unwrap_or_default()
        }
    }
}

/// The open windows the person can see, front first, excluding this assistant's own window
/// and the desktop. At most `max`.
pub fn open_windows_except_self(max: usize) -> Vec<WindowSummary> {
    let own_pid = std::process::id();
    let mut out = Vec::new();
    for hwnd in top_level_windows() {
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
        if pid == own_pid || is_cloaked(hwnd) {
            continue;
        }
        let title = window_title(hwnd);
        if title.trim().is_empty() || title == "Program Manager" {
            continue;
        }
        out.push(WindowSummary {
            title,
            hwnd: hwnd.0 as isize,
            app: friendly_app_name(process_image_path(pid).as_deref()),
        });
        if out.len() >= max {
            break;
        }
    }
    out
}

/// The computer's local date and time, already formatted for the model's instructions.
pub fn local_now() -> String {
    let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
    crate::agent::prompt::format_local_time(
        t.wYear,
        t.wMonth,
        t.wDayOfWeek,
        t.wDay,
        t.wHour,
        t.wMinute,
    )
}

/// Executable registered to open `https:` links, i.e. the user's default browser.
pub fn default_browser_exe() -> Option<PathBuf> {
    let mut buf = vec![0u16; 1024];
    let mut size = buf.len() as u32;
    let hr = unsafe {
        AssocQueryStringW(
            ASSOCF_IS_PROTOCOL | ASSOCF_NOTRUNCATE,
            ASSOCSTR_EXECUTABLE,
            w!("https"),
            w!("open"),
            Some(PWSTR(buf.as_mut_ptr())),
            &mut size,
        )
    };
    if hr.is_err() || size == 0 {
        return None;
    }
    // `size` includes the terminating NUL.
    let end = buf.iter().position(|&c| c == 0).unwrap_or(size as usize);
    Some(PathBuf::from(String::from_utf16_lossy(&buf[..end])))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn own_process_path_resolves() {
        let path = process_image_path(std::process::id()).expect("own image path");
        assert!(path.is_absolute());
    }

    /// Machine-dependent, so ignored by default: `cargo test -- --ignored`.
    #[test]
    #[ignore = "needs a default browser installed"]
    fn default_browser_resolves_to_existing_exe() {
        let exe = default_browser_exe().expect("default browser registered");
        println!("default browser: {}", exe.display());
        assert!(exe.is_file());
    }

    #[test]
    fn friendly_names() {
        use std::path::Path;
        assert_eq!(
            friendly_app_name(Some(Path::new(r"C:\x\msedge.exe"))),
            "Microsoft Edge"
        );
        assert_eq!(
            friendly_app_name(Some(Path::new(r"C:\x\WINWORD.EXE"))),
            "Word"
        );
        assert_eq!(friendly_app_name(Some(Path::new(r"C:\x\zoom.exe"))), "Zoom");
        assert_eq!(friendly_app_name(None), "an app");
    }

    #[test]
    fn listing_windows_never_includes_this_process() {
        // The test process has no windows, so this just checks the call is safe and bounded.
        assert!(open_windows_except_self(6).len() <= 6);
    }

    #[test]
    fn invalid_pid_is_none() {
        assert!(process_image_path(0).is_none());
    }
}
