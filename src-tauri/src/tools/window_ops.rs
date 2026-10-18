//! Switching to, minimizing, maximizing, restoring and closing other programs' windows.
//!
//! Closing only *asks* the program to close (the same as its X button), so a program with
//! unsaved work shows its own "Save?" question. Never this assistant's own window.

use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowThreadProcessId, IsIconic, IsWindow, IsWindowVisible, IsZoomed,
    PostMessageW, SW_MAXIMIZE, SW_MINIMIZE, SW_RESTORE, SetForegroundWindow, ShowWindow, WM_CLOSE,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowAction {
    SwitchTo,
    Minimize,
    Maximize,
    Restore,
    Close,
}

impl WindowAction {
    pub fn verb(self) -> &'static str {
        match self {
            WindowAction::SwitchTo => "Switching to",
            WindowAction::Minimize => "Minimizing",
            WindowAction::Maximize => "Making full size:",
            WindowAction::Restore => "Restoring",
            WindowAction::Close => "Closing",
        }
    }
}

fn handle(raw: isize) -> HWND {
    HWND(raw as *mut core::ffi::c_void)
}

/// Does it. Returns whether the result could be confirmed (e.g. the window is now in front).
pub fn apply(raw: isize, action: WindowAction) -> Result<bool, String> {
    let hwnd = handle(raw);
    if !unsafe { IsWindow(Some(hwnd)) }.as_bool() {
        return Err("That window has already closed.".into());
    }
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    if pid == std::process::id() {
        return Err("That's the assistant's own window.".into());
    }
    let wait = |check: &dyn Fn() -> bool| {
        let deadline = Instant::now() + Duration::from_millis(1500);
        while Instant::now() < deadline {
            if check() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        check()
    };
    Ok(match action {
        WindowAction::SwitchTo => {
            if unsafe { IsIconic(hwnd) }.as_bool() {
                let _ = unsafe { ShowWindow(hwnd, SW_RESTORE) };
            }
            let _ = unsafe { SetForegroundWindow(hwnd) };
            wait(&|| unsafe { GetForegroundWindow() } == hwnd)
        }
        WindowAction::Minimize => {
            let _ = unsafe { ShowWindow(hwnd, SW_MINIMIZE) };
            wait(&|| unsafe { IsIconic(hwnd) }.as_bool())
        }
        WindowAction::Maximize => {
            let _ = unsafe { ShowWindow(hwnd, SW_MAXIMIZE) };
            wait(&|| unsafe { IsZoomed(hwnd) }.as_bool())
        }
        WindowAction::Restore => {
            let _ = unsafe { ShowWindow(hwnd, SW_RESTORE) };
            wait(&|| !unsafe { IsIconic(hwnd) }.as_bool() && !unsafe { IsZoomed(hwnd) }.as_bool())
        }
        WindowAction::Close => {
            unsafe { PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0)) }
                .map_err(|_| "Windows didn't pass on the request to close it.".to_string())?;
            wait(&|| {
                !unsafe { IsWindow(Some(hwnd)) }.as_bool()
                    || !unsafe { IsWindowVisible(hwnd) }.as_bool()
            })
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// VISIBLE: starts its own Character Map (no tabs, nothing to save), finds that window by
    /// its process id, minimizes, restores, maximizes and closes only it.
    /// cargo test --manifest-path src-tauri/Cargo.toml live_window_ops -- --ignored --nocapture --test-threads=1
    #[test]
    #[ignore = "opens and closes a Character Map window"]
    fn live_window_ops() {
        let mut child = std::process::Command::new("charmap.exe").spawn().unwrap();
        let pid = child.id();
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut found = None;
        while found.is_none() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(300));
            found = super::super::win::top_level_windows()
                .into_iter()
                .find(|&h| {
                    let mut p = 0u32;
                    unsafe { GetWindowThreadProcessId(h, Some(&mut p)) };
                    p == pid
                });
        }
        let hwnd = found.expect("the test's own Character Map window").0 as isize;
        for a in [
            WindowAction::Minimize,
            WindowAction::Restore,
            WindowAction::Maximize,
            WindowAction::Restore,
            WindowAction::Close,
        ] {
            let r = apply(hwnd, a);
            println!("{a:?}: {r:?}");
            assert_eq!(r, Ok(true), "{a:?}");
        }
        let _ = child.wait();
    }
}
