//! Thin, read-only wrappers over Win32 used by the tools. No input simulation here.

use std::path::PathBuf;

use windows::Win32::Foundation::{CloseHandle, HWND};
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows::Win32::UI::Shell::{
    ASSOCF_IS_PROTOCOL, ASSOCF_NOTRUNCATE, ASSOCSTR_EXECUTABLE, AssocQueryStringW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId,
};
use windows::core::{PWSTR, w};

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
    fn invalid_pid_is_none() {
        assert!(process_image_path(0).is_none());
    }
}
