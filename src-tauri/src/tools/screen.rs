//! Making the screen easier to see, using only standard Windows mechanisms:
//! - Windows Magnifier through its own global shortcuts (Windows + Plus / Minus / Esc), which
//!   Windows handles itself whatever window is in front, so nothing is typed into any app;
//! - opening the exact Windows Settings page for permanent changes (the person decides there).
//!
//! There is no supported API for an app to change display scaling directly, so we don't.

use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT, KEYEVENTF_KEYUP, SendInput,
    VIRTUAL_KEY, VK_ESCAPE, VK_LWIN, VK_OEM_MINUS, VK_OEM_PLUS,
};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use windows::core::{HSTRING, w};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MagnifierAction {
    ZoomIn,
    ZoomOut,
    TurnOff,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SettingsPage {
    TextSize,
    Display,
    Magnifier,
    MousePointer,
}

impl SettingsPage {
    /// Fixed Windows Settings addresses; the model only ever picks one of these names.
    pub fn uri(self) -> &'static str {
        match self {
            SettingsPage::TextSize => "ms-settings:easeofaccess-display",
            SettingsPage::Display => "ms-settings:display",
            SettingsPage::Magnifier => "ms-settings:easeofaccess-magnifier",
            SettingsPage::MousePointer => "ms-settings:easeofaccess-mousepointer",
        }
    }

    pub fn plain_name(self) -> &'static str {
        match self {
            SettingsPage::TextSize => "Text size",
            SettingsPage::Display => "Display (scale)",
            SettingsPage::Magnifier => "Magnifier",
            SettingsPage::MousePointer => "Mouse pointer size",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MagnifierResult {
    /// Magnifier is running after the action.
    pub running: bool,
    /// The observed state matches what the action should produce.
    pub verified: bool,
}

/// Is a process with this executable name (e.g. "magnify.exe") running?
pub fn process_running(exe_name: &str) -> bool {
    let Ok(snap) = (unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }) else {
        return false;
    };
    let mut entry = PROCESSENTRY32W {
        dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    let mut found = false;
    let mut ok = unsafe { Process32FirstW(snap, &mut entry) }.is_ok();
    while ok {
        let len = entry
            .szExeFile
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(entry.szExeFile.len());
        if String::from_utf16_lossy(&entry.szExeFile[..len]).eq_ignore_ascii_case(exe_name) {
            found = true;
            break;
        }
        ok = unsafe { Process32NextW(snap, &mut entry) }.is_ok();
    }
    let _ = unsafe { CloseHandle(snap) };
    found
}

fn key(vk: VIRTUAL_KEY, up: bool) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: 0,
                dwFlags: if up {
                    KEYEVENTF_KEYUP
                } else {
                    KEYBD_EVENT_FLAGS(0)
                },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

/// Windows + `vk`: a system-wide shortcut handled by Windows, not typed into any window.
pub fn windows_shortcut(vk: VIRTUAL_KEY) -> Vec<INPUT> {
    vec![
        key(VK_LWIN, false),
        key(vk, false),
        key(vk, true),
        key(VK_LWIN, true),
    ]
}

fn wait_for(expected_running: bool, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        let running = process_running("magnify.exe");
        if running == expected_running {
            return running;
        }
        if Instant::now() >= deadline {
            return running;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

pub fn magnifier(action: MagnifierAction) -> Result<MagnifierResult, String> {
    let was_running = process_running("magnify.exe");
    let vk = match action {
        MagnifierAction::ZoomIn => VK_OEM_PLUS,
        MagnifierAction::ZoomOut if !was_running => {
            return Err("The magnifier isn't on, so there's nothing to zoom out of.".into());
        }
        MagnifierAction::ZoomOut => VK_OEM_MINUS,
        MagnifierAction::TurnOff if !was_running => {
            return Ok(MagnifierResult {
                running: false,
                verified: true,
            });
        }
        MagnifierAction::TurnOff => VK_ESCAPE,
    };
    let inputs = windows_shortcut(vk);
    let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
    if sent as usize != inputs.len() {
        return Err("Windows didn't accept the magnifier shortcut.".into());
    }
    let expect_running = action != MagnifierAction::TurnOff;
    let running = wait_for(expect_running, Duration::from_secs(4));
    Ok(MagnifierResult {
        running,
        verified: running == expect_running,
    })
}

/// Opens a fixed Windows Settings page. Verified by the Settings app running afterwards.
pub fn open_settings_page(page: SettingsPage) -> Result<bool, String> {
    let r = unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            &HSTRING::from(page.uri()),
            None,
            None,
            SW_SHOWNORMAL,
        )
    };
    // ShellExecute returns a value greater than 32 on success.
    if (r.0 as isize) <= 32 {
        return Err("Windows couldn't open that Settings page.".into());
    }
    let deadline = Instant::now() + Duration::from_secs(6);
    while Instant::now() < deadline {
        if process_running("SystemSettings.exe") {
            return Ok(true);
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_pages_are_fixed_uris() {
        for p in [
            SettingsPage::TextSize,
            SettingsPage::Display,
            SettingsPage::Magnifier,
            SettingsPage::MousePointer,
        ] {
            assert!(p.uri().starts_with("ms-settings:"), "{:?}", p);
        }
    }

    #[test]
    fn shortcuts_are_windows_key_combinations_only() {
        let inputs = windows_shortcut(VK_OEM_PLUS);
        assert_eq!(inputs.len(), 4);
        let first = unsafe { inputs[0].Anonymous.ki };
        let last = unsafe { inputs[3].Anonymous.ki };
        assert_eq!(first.wVk, VK_LWIN);
        assert_eq!(last.wVk, VK_LWIN);
        assert!(
            last.dwFlags.contains(KEYEVENTF_KEYUP),
            "Windows key is released at the end"
        );
    }

    #[test]
    fn process_check_finds_this_test() {
        let me = std::env::current_exe().unwrap();
        let name = me.file_name().unwrap().to_string_lossy().to_string();
        assert!(process_running(&name));
        assert!(!process_running("definitely-not-running-xyz.exe"));
    }

    /// VISIBLE: turns Windows Magnifier on (zooms the screen), then off again.
    /// cargo test --manifest-path src-tauri/Cargo.toml live_magnifier -- --ignored --nocapture
    #[test]
    #[ignore = "zooms the real screen briefly"]
    fn live_magnifier_on_then_off() {
        let on = magnifier(MagnifierAction::ZoomIn).unwrap();
        println!("on: {on:?}");
        std::thread::sleep(Duration::from_secs(2));
        let off = magnifier(MagnifierAction::TurnOff).unwrap();
        println!("off: {off:?}");
        assert!(on.verified && on.running);
        assert!(off.verified && !off.running);
    }
}
