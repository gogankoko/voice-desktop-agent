//! A keyboard shortcut that works anywhere: bring the assistant forward and start listening.
//!
//! Registered with Windows' own `RegisterHotKey` (no keyboard hook: the app never sees any
//! other keys). Shortcuts can already belong to another program, so the first free one of a
//! short list is used, and the app shows which. If none is free, it's logged and the app
//! works as before.

use std::sync::OnceLock;
use std::sync::mpsc;
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    HOT_KEY_MODIFIERS, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, RegisterHotKey, VIRTUAL_KEY,
    VK_A, VK_SPACE, VK_V,
};

const CANDIDATES: [(&str, HOT_KEY_MODIFIERS, VIRTUAL_KEY); 4] = [
    (
        "Ctrl+Alt+Space",
        HOT_KEY_MODIFIERS(MOD_CONTROL.0 | MOD_ALT.0),
        VK_SPACE,
    ),
    (
        "Ctrl+Alt+V",
        HOT_KEY_MODIFIERS(MOD_CONTROL.0 | MOD_ALT.0),
        VK_V,
    ),
    (
        "Ctrl+Shift+Space",
        HOT_KEY_MODIFIERS(MOD_CONTROL.0 | MOD_SHIFT.0),
        VK_SPACE,
    ),
    (
        "Ctrl+Alt+A",
        HOT_KEY_MODIFIERS(MOD_CONTROL.0 | MOD_ALT.0),
        VK_A,
    ),
];

static REGISTERED: OnceLock<Option<&'static str>> = OnceLock::new();

/// The shortcut in use, e.g. "Ctrl+Alt+Space" (None: none was free, or not started).
pub fn registered() -> Option<&'static str> {
    REGISTERED.get().copied().flatten()
}

/// Registers the first free shortcut (waits up to a second so the window can show it).
pub fn start(app: AppHandle) {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        use windows::Win32::UI::WindowsAndMessaging::{GetMessageW, MSG, WM_HOTKEY};
        const ID: i32 = 1;
        let chosen = CANDIDATES.iter().find(|(_, mods, vk)| {
            unsafe { RegisterHotKey(None, ID, *mods | MOD_NOREPEAT, vk.0.into()) }.is_ok()
        });
        let _ = tx.send(chosen.map(|(name, _, _)| *name));
        if chosen.is_none() {
            return;
        }
        let mut msg = MSG::default();
        // The registration belongs to this thread, so this thread waits for its messages.
        while unsafe { GetMessageW(&mut msg, None, 0, 0) }.as_bool() {
            if msg.message == WM_HOTKEY && msg.wParam.0 == ID as usize {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.unminimize();
                    let _ = w.show();
                    // Pressing a registered shortcut lets this app come to the front.
                    let _ = w.set_focus();
                }
                let _ = app.emit("hotkey-talk", ());
            }
        }
    });
    let chosen = rx.recv_timeout(Duration::from_secs(1)).ok().flatten();
    match chosen {
        Some(name) => crate::diag::log("hotkey", format!("talk shortcut is {name}")),
        None => crate::diag::log("hotkey", "every talk shortcut is used by another program"),
    }
    let _ = REGISTERED.set(chosen);
}
