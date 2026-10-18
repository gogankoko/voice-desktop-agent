//! Apps installed on the computer (the ones in the Start menu), opened by name, and the
//! keyboard's media keys.
//!
//! Only apps that appear in the Start menu can be opened, and never command lines,
//! administrative tools, installers or uninstallers (see `is_blocked_app`). An app is
//! started through the Start menu's own entry (`shell:AppsFolder\<id>`), with no arguments.

use std::time::{Duration, Instant};

use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoTaskMemFree};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT, KEYEVENTF_KEYUP, SendInput,
    VIRTUAL_KEY, VK_MEDIA_NEXT_TRACK, VK_MEDIA_PLAY_PAUSE, VK_MEDIA_PREV_TRACK, VK_VOLUME_DOWN,
    VK_VOLUME_MUTE, VK_VOLUME_UP,
};
use windows::Win32::UI::Shell::{
    BHID_EnumItems, FOLDERID_AppsFolder, IEnumShellItems, IShellItem, KF_FLAG_DEFAULT,
    SHGetKnownFolderItem, SIGDN, SIGDN_NORMALDISPLAY, SIGDN_PARENTRELATIVEPARSING, ShellExecuteW,
};
use windows::Win32::UI::WindowsAndMessaging::{GetWindowThreadProcessId, SW_SHOWNORMAL};
use windows::core::{HSTRING, w};

use super::win;

const MAX_QUERY_CHARS: usize = 60;
const VERIFY_TIMEOUT: Duration = Duration::from_secs(12);
const POLL: Duration = Duration::from_millis(300);

/// Words in a Start-menu name that mark an app the assistant must never open: command
/// lines, administrative and security tools, installers and uninstallers.
const BLOCKED_NAME_WORDS: &[&str] = &[
    "command prompt",
    "powershell",
    "terminal",
    "registry",
    "regedit",
    "task scheduler",
    "task manager",
    "computer management",
    "services",
    "event viewer",
    "disk management",
    "system configuration",
    "windows tools",
    "administrative tools",
    "group policy",
    "security policy",
    "resource monitor",
    "performance monitor",
    "component services",
    "odbc",
    "iscsi",
    "recovery",
    "uninstall",
    "setup",
    "install",
    "installer",
    "bash",
    "wsl",
    "ubuntu",
    "debian",
    "kali",
    "linux",
    "developer command",
    "developer powershell",
    "python",
    "node js",
    "control panel",
    "windows security",
    "firewall",
    "defragment",
    "memory diagnostic",
    "remote desktop",
    "hyper v",
    "sandbox",
    "print management",
    "credential",
];

/// Executables and file types that are never opened, whatever the Start menu calls them.
const BLOCKED_ID_ENDINGS: &[&str] = &[
    "cmd.exe",
    "powershell.exe",
    "powershell_ise.exe",
    "pwsh.exe",
    "wt.exe",
    "windowsterminal.exe",
    "regedit.exe",
    "mmc.exe",
    "msconfig.exe",
    "taskmgr.exe",
    "control.exe",
    "wsl.exe",
    "bash.exe",
    "mstsc.exe",
    ".msc",
    ".bat",
    ".cmd",
    ".ps1",
    ".vbs",
    ".js",
    ".msi",
    ".url",
];

/// An app from the Start menu. `id` stays in Rust.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledApp {
    pub name: String,
    pub id: String,
}

/// Lower-case letters and digits, single spaces.
fn normalise(s: &str) -> String {
    s.to_lowercase()
        .replace(|c: char| !c.is_alphanumeric(), " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Deterministic: must this app never be opened by the assistant?
pub fn is_blocked_app(name: &str, id: &str) -> bool {
    let padded = format!(" {} ", normalise(name));
    let id = id.to_lowercase();
    BLOCKED_NAME_WORDS
        .iter()
        .any(|w| padded.contains(&format!(" {w} ")))
        || BLOCKED_ID_ENDINGS.iter().any(|e| id.ends_with(e))
}

/// Checks the name the model asked for: a short plain name, nothing that looks like a path,
/// a command or arguments.
pub fn validate_query(query: &str) -> Result<String, String> {
    let q = query.trim();
    if q.is_empty() || q.chars().count() > MAX_QUERY_CHARS {
        return Err(format!("`name` must be 1 to {MAX_QUERY_CHARS} characters."));
    }
    if q.chars()
        .any(|c| c.is_control() || "\\/:*?\"<>|%&;$`".contains(c))
    {
        return Err("`name` must be just the app's name, like 'Spotify'.".into());
    }
    Ok(q.to_string())
}

/// What a name request matched.
#[derive(Debug, PartialEq, Eq)]
pub enum Match<'a> {
    One(&'a InstalledApp),
    Several(Vec<&'a InstalledApp>),
    /// Only blocked apps had that name.
    Blocked(String),
    None,
}

/// Finds the app the person meant: an exact name first, then names starting with it, then
/// names containing all its words. Blocked apps are never returned.
pub fn find_app<'a>(apps: &'a [InstalledApp], query: &str) -> Match<'a> {
    let q = normalise(query);
    if q.is_empty() {
        return Match::None;
    }
    let words: Vec<&str> = q.split(' ').collect();
    let tiers: [&dyn Fn(&str) -> bool; 3] = [
        &|n: &str| n == q,
        &|n: &str| n.starts_with(&q),
        &|n: &str| {
            words
                .iter()
                .all(|w| n.split(' ').any(|nw| nw.starts_with(w)))
        },
    ];
    for tier in tiers {
        let hits: Vec<&InstalledApp> = apps.iter().filter(|a| tier(&normalise(&a.name))).collect();
        if hits.is_empty() {
            continue;
        }
        let allowed: Vec<&InstalledApp> = hits
            .iter()
            .copied()
            .filter(|a| !is_blocked_app(&a.name, &a.id))
            .collect();
        let mut unique: Vec<&InstalledApp> = Vec::new();
        for a in allowed {
            if !unique.iter().any(|u| u.name.eq_ignore_ascii_case(&a.name)) {
                unique.push(a);
            }
        }
        return match unique.len() {
            0 => Match::Blocked(hits[0].name.clone()),
            1 => Match::One(unique[0]),
            _ => Match::Several(unique.into_iter().take(6).collect()),
        };
    }
    Match::None
}

fn display_name(item: &IShellItem, kind: SIGDN) -> Option<String> {
    let p = unsafe { item.GetDisplayName(kind) }.ok()?;
    let s = unsafe { p.to_string() }.ok();
    unsafe { CoTaskMemFree(Some(p.0 as *const _)) };
    s
}

/// Every app in the Start menu ("All apps").
pub fn installed_apps() -> Result<Vec<InstalledApp>, String> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }
    let fail = |_| "I couldn't look at the apps on this computer.".to_string();
    let folder: IShellItem =
        unsafe { SHGetKnownFolderItem(&FOLDERID_AppsFolder, KF_FLAG_DEFAULT, None) }
            .map_err(fail)?;
    let items: IEnumShellItems =
        unsafe { folder.BindToHandler(None, &BHID_EnumItems) }.map_err(fail)?;
    let mut out = Vec::new();
    loop {
        let mut one: [Option<IShellItem>; 1] = [None];
        let mut fetched = 0u32;
        if unsafe { items.Next(&mut one, Some(&mut fetched)) }.is_err() || fetched == 0 {
            break;
        }
        let Some(item) = one[0].take() else { break };
        if let (Some(name), Some(id)) = (
            display_name(&item, SIGDN_NORMALDISPLAY),
            display_name(&item, SIGDN_PARENTRELATIVEPARSING),
        ) {
            if !name.trim().is_empty() && !id.trim().is_empty() {
                out.push(InstalledApp { name, id });
            }
        }
        if out.len() >= 2000 {
            break;
        }
    }
    Ok(out)
}

#[derive(Debug, Clone, PartialEq)]
pub struct Launched {
    pub name: String,
    /// A window of the app came to the front.
    pub verified: bool,
    /// That window, for operating it afterwards. Never sent to the model.
    pub hwnd: Option<isize>,
}

/// Does a window (title, exe) look like it belongs to the app called `name`?
pub fn looks_like_app_window(name: &str, title: &str, exe_stem: &str) -> bool {
    let n = normalise(name);
    let compact = n.replace(' ', "");
    let stem = normalise(exe_stem).replace(' ', "");
    let first = n.split(' ').next().unwrap_or_default();
    (!stem.is_empty() && (stem == compact || (first.len() >= 4 && stem.contains(first))))
        || normalise(title).contains(&n)
}

/// Opens the app the person named, if it's in the Start menu and not blocked.
pub fn open_by_name(query: &str) -> Result<Launched, String> {
    let query = validate_query(query)?;
    let apps = installed_apps()?;
    let app = match find_app(&apps, &query) {
        Match::One(a) => a.clone(),
        Match::Several(list) => {
            let names: Vec<&str> = list.iter().map(|a| a.name.as_str()).collect();
            return Err(format!(
                "Several apps match '{query}': {}. Ask the person which one.",
                names.join(", ")
            ));
        }
        Match::Blocked(name) => {
            return Err(format!(
                "'{name}' is a system or administrative tool, so for safety I don't open it. The person can open it themselves."
            ));
        }
        Match::None => {
            return Err(format!(
                "I couldn't find an app called '{query}' in the Start menu. It may not be installed."
            ));
        }
    };
    let target = HSTRING::from(format!("shell:AppsFolder\\{}", app.id));
    let r = unsafe { ShellExecuteW(None, w!("open"), &target, None, None, SW_SHOWNORMAL) };
    if (r.0 as isize) <= 32 {
        crate::diag::log("apps", "open_installed_app: ShellExecute failed");
        return Err(format!(
            "I tried to open {}, but Windows didn't start it.",
            app.name
        ));
    }
    let own_pid = std::process::id();
    let deadline = Instant::now() + VERIFY_TIMEOUT;
    while Instant::now() < deadline {
        std::thread::sleep(POLL);
        let hwnd = unsafe { windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow() };
        if hwnd.is_invalid() {
            continue;
        }
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
        if pid == own_pid {
            continue;
        }
        if let Some(w) = win::foreground_window() {
            let stem = w
                .exe_path
                .as_deref()
                .and_then(|p| p.file_stem())
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();
            if looks_like_app_window(&app.name, &w.title, &stem) {
                return Ok(Launched {
                    name: app.name,
                    verified: true,
                    hwnd: Some(hwnd.0 as isize),
                });
            }
        }
    }
    Ok(Launched {
        name: app.name,
        verified: false,
        hwnd: None,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaAction {
    PlayPause,
    NextTrack,
    PreviousTrack,
    VolumeUp,
    VolumeDown,
    Mute,
}

impl MediaAction {
    pub fn plain(self) -> &'static str {
        match self {
            MediaAction::PlayPause => "Play / pause",
            MediaAction::NextTrack => "Next track",
            MediaAction::PreviousTrack => "Previous track",
            MediaAction::VolumeUp => "Volume up",
            MediaAction::VolumeDown => "Volume down",
            MediaAction::Mute => "Mute / unmute",
        }
    }

    /// The key, and how many presses (each volume press is about 2%).
    fn key(self) -> (VIRTUAL_KEY, usize) {
        match self {
            MediaAction::PlayPause => (VK_MEDIA_PLAY_PAUSE, 1),
            MediaAction::NextTrack => (VK_MEDIA_NEXT_TRACK, 1),
            MediaAction::PreviousTrack => (VK_MEDIA_PREV_TRACK, 1),
            MediaAction::VolumeUp => (VK_VOLUME_UP, 5),
            MediaAction::VolumeDown => (VK_VOLUME_DOWN, 5),
            MediaAction::Mute => (VK_VOLUME_MUTE, 1),
        }
    }
}

fn key_input(vk: VIRTUAL_KEY, up: bool) -> INPUT {
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

/// Presses a media key, like the ones on many keyboards. These go to Windows itself (the
/// playing app or the volume), never into whichever window is in front.
pub fn media_key(action: MediaAction) -> Result<(), String> {
    let (vk, presses) = action.key();
    let mut inputs = Vec::new();
    for _ in 0..presses {
        inputs.push(key_input(vk, false));
        inputs.push(key_input(vk, true));
    }
    let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
    if sent as usize != inputs.len() {
        return Err("Windows didn't accept the media key.".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(name: &str, id: &str) -> InstalledApp {
        InstalledApp {
            name: name.into(),
            id: id.into(),
        }
    }

    fn sample() -> Vec<InstalledApp> {
        vec![
            app("Spotify", r"C:\Users\x\AppData\Roaming\Spotify\Spotify.exe"),
            app("Microsoft Teams", "MSTeams_8wekyb3d8bbwe!MSTeams"),
            app("Word", "Microsoft.Office.WINWORD.EXE.15"),
            app("Visual Studio Code", "Microsoft.VisualStudioCode"),
            app("Visual Studio 2022", "VisualStudio.d1a2"),
            app(
                "Command Prompt",
                r"{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}\cmd.exe",
            ),
            app(
                "Windows PowerShell",
                r"{1AC14E77}\WindowsPowerShell\v1.0\powershell.exe",
            ),
            app("Terminal", "Microsoft.WindowsTerminal_8wekyb3d8bbwe!App"),
            app("Registry Editor", r"{F38BF404}\regedit.exe"),
            app("Uninstall Zoom", r"C:\x\Zoom\uninstall.exe"),
            app("Services", r"{1AC14E77}\services.msc"),
        ]
    }

    #[test]
    fn finds_apps_by_plain_name() {
        let apps = sample();
        assert!(matches!(find_app(&apps, "spotify"), Match::One(a) if a.name == "Spotify"));
        assert!(matches!(find_app(&apps, "Spotify."), Match::One(a) if a.name == "Spotify"));
        assert!(matches!(find_app(&apps, "teams"), Match::One(a) if a.name == "Microsoft Teams"));
        assert!(matches!(find_app(&apps, "word"), Match::One(a) if a.name == "Word"));
        assert!(matches!(find_app(&apps, "visual studio"), Match::Several(v) if v.len() == 2));
        assert_eq!(find_app(&apps, "photoshop"), Match::None);
    }

    #[test]
    fn never_opens_command_lines_or_admin_tools() {
        let apps = sample();
        for q in [
            "command prompt",
            "powershell",
            "terminal",
            "registry editor",
            "uninstall zoom",
            "services",
        ] {
            assert!(matches!(find_app(&apps, q), Match::Blocked(_)), "{q}");
        }
        assert!(is_blocked_app("Totally Normal", r"C:\x\cmd.exe"));
        assert!(is_blocked_app("Thing", r"C:\x\script.BAT"));
        assert!(!is_blocked_app("Spotify", r"C:\x\Spotify.exe"));
        assert!(!is_blocked_app(
            "Calculator",
            "Microsoft.WindowsCalculator_8wekyb3d8bbwe!App"
        ));
    }

    #[test]
    fn query_must_be_a_plain_name() {
        assert_eq!(validate_query("  Spotify ").unwrap(), "Spotify");
        for bad in [
            "",
            r"C:\Windows\System32\cmd.exe",
            "spotify & calc",
            "cmd /c del",
            "a\nb",
            &"x".repeat(61),
        ] {
            assert!(validate_query(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn app_windows_are_recognised() {
        assert!(looks_like_app_window(
            "Spotify",
            "Yeat - Breathe",
            "Spotify"
        ));
        assert!(looks_like_app_window(
            "Microsoft Teams",
            "Chat | Microsoft Teams",
            "ms-teams"
        ));
        assert!(looks_like_app_window(
            "Visual Studio Code",
            "main.rs - Visual Studio Code",
            "Code"
        ));
        assert!(!looks_like_app_window(
            "Spotify",
            "Untitled - Notepad",
            "notepad"
        ));
    }

    #[test]
    fn media_volume_moves_in_small_steps() {
        assert_eq!(MediaAction::VolumeUp.key().1, 5);
        assert_eq!(MediaAction::PlayPause.key().1, 1);
    }

    /// Lists how many Start-menu apps there are and whether Spotify is one (names only).
    /// cargo test --manifest-path src-tauri/Cargo.toml live_installed -- --ignored --nocapture
    #[test]
    #[ignore = "reads the real Start menu"]
    fn live_installed_apps() {
        let apps = installed_apps().unwrap();
        println!("{} apps", apps.len());
        println!("spotify: {:?}", find_app(&apps, "spotify"));
        println!("cmd: {:?}", find_app(&apps, "command prompt"));
        assert!(apps.len() > 10);
    }
}
