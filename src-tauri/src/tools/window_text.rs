//! Reading the visible text of a window (for "what does this error mean?").
//!
//! Only when the person asks. Uses the accessibility tree: text labels, the contents of
//! read-only or editable text areas (never password boxes) and button names. The text goes
//! to the model marked as untrusted, like web page text.

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Accessibility::{
    IUIAutomationTextPattern, IUIAutomationValuePattern, TreeScope_Descendants,
    UIA_ButtonControlTypeId, UIA_DocumentControlTypeId, UIA_EditControlTypeId,
    UIA_TextControlTypeId, UIA_TextPatternId, UIA_ValuePatternId,
};
use windows::Win32::UI::WindowsAndMessaging::IsWindow;

use super::browser::{automation, is_password, name_of};

const MAX_CHARS: usize = 4000;
const MAX_ELEMENTS: i32 = 600;

#[derive(Debug, Clone, PartialEq)]
pub struct WindowText {
    pub text: String,
    pub buttons: Vec<String>,
    pub truncated: bool,
}

/// Collects the window's visible text. Fails if the window has closed.
pub fn read_window(hwnd_raw: isize) -> Result<WindowText, String> {
    let hwnd = HWND(hwnd_raw as *mut core::ffi::c_void);
    if !unsafe { IsWindow(Some(hwnd)) }.as_bool() {
        return Err("That window has closed.".into());
    }
    let a = automation()?;
    let root = unsafe { a.ElementFromHandle(hwnd) }
        .map_err(|_| "I couldn't look inside that window.".to_string())?;
    let all = unsafe {
        root.FindAll(
            TreeScope_Descendants,
            &a.CreateTrueCondition()
                .map_err(|_| "I couldn't look inside that window.".to_string())?,
        )
    }
    .map_err(|_| "I couldn't look inside that window.".to_string())?;
    let n = unsafe { all.Length() }.unwrap_or(0).min(MAX_ELEMENTS);
    let mut lines: Vec<String> = Vec::new();
    let mut buttons: Vec<String> = Vec::new();
    for i in 0..n {
        let Ok(e) = (unsafe { all.GetElement(i) }) else {
            continue;
        };
        let Ok(ct) = (unsafe { e.CurrentControlType() }) else {
            continue;
        };
        if ct == UIA_ButtonControlTypeId {
            let name = name_of(&e);
            if !name.is_empty() && !buttons.contains(&name) && buttons.len() < 12 {
                buttons.push(name);
            }
        } else if ct == UIA_TextControlTypeId {
            lines.push(name_of(&e));
        } else if ct == UIA_EditControlTypeId || ct == UIA_DocumentControlTypeId {
            if is_password(&e) {
                continue;
            }
            let from_text =
                unsafe { e.GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId) }
                    .and_then(|p| unsafe { p.DocumentRange() })
                    .and_then(|r| unsafe { r.GetText(MAX_CHARS as i32) })
                    .map(|t| t.to_string());
            let value = from_text.ok().or_else(|| {
                unsafe { e.GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId) }
                    .and_then(|v| unsafe { v.CurrentValue() })
                    .map(|b| b.to_string())
                    .ok()
            });
            if let Some(v) = value {
                lines.push(v);
            }
        }
    }
    Ok(assemble(lines, buttons))
}

/// Tidies collected text: trims, drops blanks and repeats, caps the length.
pub fn assemble(lines: Vec<String>, buttons: Vec<String>) -> WindowText {
    let mut text = String::new();
    let mut last = String::new();
    for l in lines {
        let l = l.split_whitespace().collect::<Vec<_>>().join(" ");
        if l.is_empty() || l == last {
            continue;
        }
        text.push_str(&l);
        text.push('\n');
        last = l;
    }
    let truncated = text.chars().count() > MAX_CHARS;
    if truncated {
        text = text.chars().take(MAX_CHARS).collect();
    }
    WindowText {
        text,
        buttons,
        truncated,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assemble_tidies_and_caps() {
        let w = assemble(
            vec![
                "  Error 0x80070005  ".into(),
                "".into(),
                "Error 0x80070005".into(),
                "Access is denied.".into(),
            ],
            vec!["OK".into()],
        );
        assert_eq!(w.text, "Error 0x80070005\nAccess is denied.\n");
        assert_eq!(w.buttons, vec!["OK"]);
        let long = assemble(vec!["x".repeat(MAX_CHARS + 10)], vec![]);
        assert!(long.truncated && long.text.chars().count() == MAX_CHARS);
    }

    #[test]
    fn closed_windows_are_refused() {
        assert!(read_window(0x7fff_fff0).is_err());
    }

    /// Live: shows a test message box (unique title), reads it, then closes ONLY that box.
    /// cargo test --manifest-path src-tauri/Cargo.toml live_read_window -- --ignored --nocapture
    #[test]
    #[ignore = "shows a test message box briefly"]
    fn live_read_window_message_box() {
        let title = format!("Assistant Test Error {}", std::process::id());
        let script = format!(
            "Add-Type -AssemblyName PresentationFramework; [System.Windows.MessageBox]::Show('Error 0x80070005: Access is denied. The file could not be saved.', '{title}', 'OK', 'Error')"
        );
        let mut child = std::process::Command::new("powershell.exe")
            .args(["-NoProfile", "-Command", &script])
            .spawn()
            .unwrap();
        let mut found = None;
        for _ in 0..40 {
            std::thread::sleep(std::time::Duration::from_millis(250));
            found = super::super::win::open_windows_except_self(20)
                .into_iter()
                .find(|w| w.title == title);
            if found.is_some() {
                break;
            }
        }
        let w = found.expect("test message box appears");
        let text = read_window(w.hwnd);
        println!("read: {text:?}");
        // Close only the window this test created (exact unique title), then end its process.
        let _ = child.kill();
        let text = text.unwrap();
        assert!(text.text.contains("0x80070005"));
        assert!(text.buttons.iter().any(|b| b == "OK"));
    }
}
