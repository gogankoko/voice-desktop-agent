//! Browser control through Windows UI Automation (the accessibility interface).
//!
//! Works with the user's normal default browser, without extensions, debugging ports or
//! keystrokes: pages are read with the Text pattern, scrolled with the Scroll pattern,
//! and links are followed with the Invoke pattern. Every action re-finds its target and
//! checks it is still what was observed (same page, same link text) before acting.
//!
//! All functions are blocking and must run off the UI thread (they initialise COM).

use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
};
use windows::Win32::System::Variant::{VARIANT, VT_I4};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationCondition, IUIAutomationElement,
    IUIAutomationInvokePattern, IUIAutomationScrollPattern, IUIAutomationTextPattern,
    IUIAutomationValuePattern, ScrollAmount_LargeDecrement, ScrollAmount_LargeIncrement,
    ScrollAmount_NoAmount, TreeScope_Descendants, UIA_ButtonControlTypeId, UIA_CONTROLTYPE_ID,
    UIA_ComboBoxControlTypeId, UIA_ControlTypePropertyId, UIA_DocumentControlTypeId,
    UIA_EditControlTypeId, UIA_HyperlinkControlTypeId, UIA_InvokePatternId, UIA_ScrollPatternId,
    UIA_TextPatternId, UIA_ValuePatternId,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT, KEYEVENTF_KEYUP,
    KEYEVENTF_UNICODE, SendInput, VIRTUAL_KEY, VK_CONTROL, VK_DELETE, VK_OEM_MINUS, VK_OEM_PLUS,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowThreadProcessId, SetForegroundWindow,
};

use super::web_url::SafeUrl;
use super::win::{process_image_path, top_level_windows};

const MAX_PAGE_CHARS: usize = 6000;
const MAX_LINKS: usize = 40;
/// Read-aloud text goes to the app's own voice, never to the model, so it can be longer.
const MAX_READ_ALOUD_CHARS: usize = 60_000;
const MAX_CONTROLS: usize = 60;

/// Field labels the assistant must never type into, whatever it is asked.
const SENSITIVE_FIELD_WORDS: &[&str] = &[
    "password",
    "passcode",
    "pin",
    "card",
    "card number",
    "cvv",
    "cvc",
    "security code",
    "expiry",
    "expiration",
    "iban",
    "sort code",
    "account number",
    "routing",
    "ssn",
    "social security",
    "one-time code",
    "otp",
    "verification code",
];
const NAVIGATION_TIMEOUT: Duration = Duration::from_secs(10);
const POLL: Duration = Duration::from_millis(300);
/// How long to wait for a page that is still loading before giving up on it.
const PAGE_READY_TIMEOUT: Duration = Duration::from_secs(8);

#[derive(Debug, Clone, PartialEq)]
pub struct PageText {
    pub title: String,
    pub address: Option<String>,
    pub text: String,
    pub truncated: bool,
}

/// A link as observed; enough to find the same link again and check it hasn't changed.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LinkRef {
    pub text: String,
    /// Which link with this exact text (0 = first), counting in page order.
    pub ordinal: usize,
    /// The page address when the link was observed. Following fails if the page changed.
    pub page_address: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NavigationResult {
    /// The browser's address bar showed the expected site (or, for links, a new page).
    pub verified: bool,
    pub address: Option<String>,
}

fn automation() -> Result<IUIAutomation, String> {
    unsafe {
        // S_FALSE / RPC_E_CHANGED_MODE just mean COM is already initialised on this thread.
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)
    }
    .map_err(|e| {
        crate::diag::log("browser", format!("UI Automation unavailable: {e}"));
        "Windows' accessibility interface isn't available, so I can't work with the browser."
            .to_string()
    })
}

fn int_variant(v: i32) -> VARIANT {
    let mut var = VARIANT::default();
    unsafe {
        (*var.Anonymous.Anonymous).vt = VT_I4;
        (*var.Anonymous.Anonymous).Anonymous.lVal = v;
    }
    var
}

fn control_type(
    a: &IUIAutomation,
    ct: UIA_CONTROLTYPE_ID,
) -> Result<IUIAutomationCondition, String> {
    unsafe { a.CreatePropertyCondition(UIA_ControlTypePropertyId, &int_variant(ct.0)) }
        .map_err(|_| "Couldn't search the browser window.".to_string())
}

fn same_exe(a: &Path, b: &Path) -> bool {
    a.as_os_str()
        .to_string_lossy()
        .eq_ignore_ascii_case(&b.as_os_str().to_string_lossy())
}

/// The front-most window of the given browser executable.
fn browser_window(exe: &Path) -> Option<HWND> {
    top_level_windows().into_iter().find(|&hwnd| {
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
        process_image_path(pid).is_some_and(|p| same_exe(&p, exe))
    })
}

struct Browser {
    a: IUIAutomation,
    hwnd: HWND,
    root: IUIAutomationElement,
}

const NO_BROWSER: &str = "I can't find your web browser window. Ask me to open a website first.";

impl Browser {
    fn find(exe: &Path) -> Result<Self, String> {
        let a = automation()?;
        let hwnd = browser_window(exe).ok_or_else(|| NO_BROWSER.to_string())?;
        let root = unsafe { a.ElementFromHandle(hwnd) }.map_err(|_| NO_BROWSER.to_string())?;
        Ok(Self { a, hwnd, root })
    }

    /// The web page itself (Chromium-based browsers and Firefox expose it as a Document).
    fn document(&self) -> Result<IUIAutomationElement, String> {
        let cond = control_type(&self.a, UIA_DocumentControlTypeId)?;
        // Right after navigating, the page may not exist in the accessibility tree yet.
        let deadline = Instant::now() + PAGE_READY_TIMEOUT;
        loop {
            if let Ok(doc) = unsafe { self.root.FindFirst(TreeScope_Descendants, &cond) } {
                return Ok(doc);
            }
            if Instant::now() >= deadline {
                return Err("I can't see a web page in the browser right now.".to_string());
            }
            std::thread::sleep(POLL);
        }
    }

    /// What the address bar shows, if it can be found.
    fn address(&self) -> Option<String> {
        let cond = control_type(&self.a, UIA_EditControlTypeId).ok()?;
        let edits = unsafe { self.root.FindAll(TreeScope_Descendants, &cond) }.ok()?;
        for i in 0..unsafe { edits.Length() }.unwrap_or(0) {
            let Ok(e) = (unsafe { edits.GetElement(i) }) else {
                continue;
            };
            let name = unsafe { e.CurrentName() }
                .map(|b| b.to_string())
                .unwrap_or_default();
            if !name.to_lowercase().contains("address") {
                continue;
            }
            if let Ok(v) =
                unsafe { e.GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId) }
            {
                if let Ok(val) = unsafe { v.CurrentValue() } {
                    let s = val.to_string();
                    if !s.trim().is_empty() {
                        return Some(s.trim().to_string());
                    }
                }
            }
        }
        None
    }

    fn links(&self) -> Result<Vec<IUIAutomationElement>, String> {
        let doc = self.document()?;
        let cond = control_type(&self.a, UIA_HyperlinkControlTypeId)?;
        let all = unsafe { doc.FindAll(TreeScope_Descendants, &cond) }
            .map_err(|_| "I couldn't look for links on this page.".to_string())?;
        let n = unsafe { all.Length() }.unwrap_or(0);
        Ok((0..n)
            .filter_map(|i| unsafe { all.GetElement(i) }.ok())
            .collect())
    }
}

fn name_of(e: &IUIAutomationElement) -> String {
    let raw = unsafe { e.CurrentName() }
        .map(|b| b.to_string())
        .unwrap_or_default();
    raw.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn address_matches(address: &str, site: &str) -> bool {
    let a = address.to_lowercase();
    let host_part = a
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_start_matches("www.");
    let host = host_part.split(['/', '?', '#']).next().unwrap_or("");
    host == site || host.ends_with(&format!(".{site}"))
}

/// After navigating: waits (bounded) until the page has a title and some text, so the next
/// step doesn't look at a half-loaded page. Never fails; a slow page is simply left as is.
fn wait_until_loaded(exe: &Path) {
    let deadline = Instant::now() + PAGE_READY_TIMEOUT;
    while Instant::now() < deadline {
        if let Ok(b) = Browser::find(exe) {
            let cond = control_type(&b.a, UIA_DocumentControlTypeId);
            let doc = cond
                .ok()
                .and_then(|c| unsafe { b.root.FindFirst(TreeScope_Descendants, &c) }.ok());
            if let Some(doc) = doc {
                let has_title = !name_of(&doc).is_empty();
                let has_text = unsafe {
                    doc.GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId)
                }
                .and_then(|p| unsafe { p.DocumentRange() })
                .and_then(|r| unsafe { r.GetText(200) })
                .map(|t| !t.to_string().trim().is_empty())
                .unwrap_or(false);
                if has_title && has_text {
                    return;
                }
            }
        }
        std::thread::sleep(POLL);
    }
}

/// Opens `url` in the browser (a new tab), then waits for the address bar to show the site.
pub fn open_website(exe: &Path, url: &SafeUrl) -> Result<NavigationResult, String> {
    // `url.url` always starts with "https://", so it can't be read as a command-line flag.
    Command::new(exe).arg(&url.url).spawn().map_err(|e| {
        crate::diag::log("browser", format!("open_website: launch failed: {e}"));
        "I couldn't ask your browser to open that website.".to_string()
    })?;
    let deadline = Instant::now() + NAVIGATION_TIMEOUT;
    let mut last = None;
    while Instant::now() < deadline {
        if let Ok(b) = Browser::find(exe) {
            last = b.address();
            if last
                .as_deref()
                .is_some_and(|a| address_matches(a, url.site()))
            {
                wait_until_loaded(exe);
                return Ok(NavigationResult {
                    verified: true,
                    address: last,
                });
            }
        }
        std::thread::sleep(POLL);
    }
    Ok(NavigationResult {
        verified: false,
        address: last,
    })
}

fn tidy(text: &str) -> String {
    let mut out = String::new();
    for line in text.lines() {
        let line = line.split_whitespace().collect::<Vec<_>>().join(" ");
        if !line.is_empty() {
            out.push_str(&line);
            out.push('\n');
        }
    }
    out
}

pub fn read_page(exe: &Path) -> Result<PageText, String> {
    read_page_limited(exe, MAX_PAGE_CHARS)
}

/// The page's main prose for reading aloud (menus, buttons and short fragments dropped).
pub fn read_article(exe: &Path) -> Result<PageText, String> {
    let mut page = read_page_limited(exe, 200_000)?;
    let article = article_text(&page.text);
    page.truncated = article.chars().count() > MAX_READ_ALOUD_CHARS;
    page.text = article.chars().take(MAX_READ_ALOUD_CHARS).collect();
    Ok(page)
}

/// Keeps lines that read like sentences (6+ words or ending in . ! ?). Falls back to the whole
/// text if that leaves too little, e.g. on pages that are mostly short items.
pub fn article_text(text: &str) -> String {
    let prose: Vec<&str> = text
        .lines()
        .filter(|l| {
            let words = l.split_whitespace().count();
            words >= 6 || (words >= 3 && l.trim_end().ends_with(['.', '!', '?']))
        })
        .collect();
    let joined = prose.join("\n");
    if joined.chars().count() >= 120 {
        joined
    } else {
        text.to_string()
    }
}

fn read_page_limited(exe: &Path, max_chars: usize) -> Result<PageText, String> {
    let b = Browser::find(exe)?;
    let doc = b.document()?;
    let title = name_of(&doc);
    let text = unsafe { doc.GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId) }
        .and_then(|p| unsafe { p.DocumentRange() })
        .and_then(|r| unsafe { r.GetText(200_000) })
        .map(|t| tidy(&t.to_string()))
        .map_err(|_| "I couldn't read the text on this page.".to_string())?;
    let truncated = text.chars().count() > max_chars;
    let text = if truncated {
        text.chars().take(max_chars).collect()
    } else {
        text
    };
    Ok(PageText {
        title,
        address: b.address(),
        text,
        truncated,
    })
}

pub fn scroll_page(exe: &Path, down: bool) -> Result<(), String> {
    let b = Browser::find(exe)?;
    let doc = b.document()?;
    let scroll =
        unsafe { doc.GetCurrentPatternAs::<IUIAutomationScrollPattern>(UIA_ScrollPatternId) }
            .map_err(|_| "This page can't be scrolled.".to_string())?;
    let amount = if down {
        ScrollAmount_LargeIncrement
    } else {
        ScrollAmount_LargeDecrement
    };
    unsafe { scroll.Scroll(ScrollAmount_NoAmount, amount) }.map_err(|_| {
        if down {
            "I couldn't scroll down. This may already be the bottom of the page.".to_string()
        } else {
            "I couldn't scroll up. This may already be the top of the page.".to_string()
        }
    })
}

/// Links currently on screen (falls back to all links if none are on screen), in page order.
pub fn list_links(exe: &Path) -> Result<Vec<LinkRef>, String> {
    let b = Browser::find(exe)?;
    let page_address = b.address();
    let links = b.links()?;
    let mut seen: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let mut on_screen = Vec::new();
    let mut off_screen = Vec::new();
    for e in &links {
        let text = name_of(e);
        if text.is_empty() {
            continue;
        }
        let ordinal = *seen
            .entry(text.clone())
            .and_modify(|n| *n += 1)
            .or_insert(0);
        let link = LinkRef {
            text,
            ordinal,
            page_address: page_address.clone(),
        };
        if unsafe { e.CurrentIsOffscreen() }
            .map(|b| b.as_bool())
            .unwrap_or(false)
        {
            off_screen.push(link);
        } else {
            on_screen.push(link);
        }
    }
    let mut out = if on_screen.is_empty() {
        off_screen
    } else {
        on_screen
    };
    out.truncate(MAX_LINKS);
    Ok(out)
}

/// Re-finds the observed link (same page, same text and position) and activates it.
pub fn follow_link(exe: &Path, link: &LinkRef) -> Result<NavigationResult, String> {
    let b = Browser::find(exe)?;
    let before = b.address();
    if before != link.page_address {
        return Err("The page has changed since I looked at its links, so I didn't click anything. Ask me to list the links again.".into());
    }
    let target = b
        .links()?
        .into_iter()
        .filter(|e| name_of(e) == link.text)
        .nth(link.ordinal)
        .ok_or_else(|| {
            "That link isn't on the page any more, so I didn't click anything.".to_string()
        })?;
    let invoke =
        unsafe { target.GetCurrentPatternAs::<IUIAutomationInvokePattern>(UIA_InvokePatternId) }
            .map_err(|_| "That link can't be clicked from here.".to_string())?;
    unsafe { invoke.Invoke() }.map_err(|_| "Clicking that link didn't work.".to_string())?;

    let deadline = Instant::now() + NAVIGATION_TIMEOUT;
    let mut after = before.clone();
    while Instant::now() < deadline {
        std::thread::sleep(POLL);
        if let Ok(b) = Browser::find(exe) {
            after = b.address();
            if after.is_some() && after != before {
                wait_until_loaded(exe);
                return Ok(NavigationResult {
                    verified: true,
                    address: after,
                });
            }
        }
    }
    Ok(NavigationResult {
        verified: false,
        address: after,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ControlKind {
    Button,
    TextField,
}

/// A button or text box as observed; enough to find it again and check nothing changed.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ControlRef {
    pub kind: ControlKind,
    pub label: String,
    /// Which control of this kind with this exact label (0 = first), in page order.
    pub ordinal: usize,
    pub page_address: Option<String>,
    /// Password or payment-like field: never typed into, value never reported.
    pub sensitive: bool,
    /// Current text of a (non-sensitive) text box.
    pub value: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PressResult {
    /// The page address changed after pressing (many buttons legitimately don't change it).
    pub address_changed: bool,
    pub address: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TypeResult {
    /// The box now contains exactly the requested text.
    pub verified: bool,
}

/// Deterministic: is this a field the assistant must never type into?
pub fn is_sensitive_label(label: &str) -> bool {
    let t = label
        .to_lowercase()
        .replace(|c: char| !c.is_alphanumeric() && c != '-', " ");
    let padded = format!(" {} ", t.split_whitespace().collect::<Vec<_>>().join(" "));
    SENSITIVE_FIELD_WORDS
        .iter()
        .any(|w| padded.contains(&format!(" {w} ")))
}

fn is_password(e: &IUIAutomationElement) -> bool {
    unsafe { e.CurrentIsPassword() }
        .map(|b| b.as_bool())
        .unwrap_or(true)
}

fn is_editable(e: &IUIAutomationElement) -> bool {
    unsafe { e.GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId) }
        .and_then(|v| unsafe { v.CurrentIsReadOnly() })
        .map(|ro| !ro.as_bool())
        .unwrap_or(false)
}

fn value_of(e: &IUIAutomationElement) -> Option<String> {
    let v =
        unsafe { e.GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId) }.ok()?;
    unsafe { v.CurrentValue() }.ok().map(|b| b.to_string())
}

impl Browser {
    /// Buttons and text boxes on the page, in page order.
    fn controls(&self) -> Result<Vec<(ControlKind, IUIAutomationElement)>, String> {
        let doc = self.document()?;
        let buttons = control_type(&self.a, UIA_ButtonControlTypeId)?;
        let edits = control_type(&self.a, UIA_EditControlTypeId)?;
        let combos = control_type(&self.a, UIA_ComboBoxControlTypeId)?;
        let either = unsafe { self.a.CreateOrCondition(&buttons, &edits) }
            .and_then(|c| unsafe { self.a.CreateOrCondition(&c, &combos) })
            .map_err(|_| "Couldn't search the page.".to_string())?;
        let all = unsafe { doc.FindAll(TreeScope_Descendants, &either) }
            .map_err(|_| "I couldn't look for buttons and boxes on this page.".to_string())?;
        let n = unsafe { all.Length() }.unwrap_or(0);
        Ok((0..n)
            .filter_map(|i| unsafe { all.GetElement(i) }.ok())
            .filter_map(|e| {
                let kind = match unsafe { e.CurrentControlType() }.ok()? {
                    t if t == UIA_ButtonControlTypeId => ControlKind::Button,
                    // Search boxes with suggestions (e.g. YouTube) are combo boxes; drop-down
                    // menus are too, but those are read-only and can't be typed into.
                    t if t == UIA_ComboBoxControlTypeId && !is_editable(&e) => return None,
                    _ => ControlKind::TextField,
                };
                Some((kind, e))
            })
            .collect())
    }

    /// Re-finds an observed control: same page, same kind, same label and position.
    fn find_control(&self, c: &ControlRef) -> Result<IUIAutomationElement, String> {
        if self.address() != c.page_address {
            return Err("The page has changed since I looked at it, so I didn't touch anything. Ask me to look at the page again.".into());
        }
        self.controls()?
            .into_iter()
            .filter(|(k, e)| *k == c.kind && name_of(e) == c.label)
            .nth(c.ordinal)
            .map(|(_, e)| e)
            .ok_or_else(|| {
                "That isn't on the page any more, so I didn't touch anything.".to_string()
            })
    }
}

/// Buttons and text boxes on the current page.
pub fn list_controls(exe: &Path) -> Result<Vec<ControlRef>, String> {
    let b = Browser::find(exe)?;
    let page_address = b.address();
    let mut seen: std::collections::HashMap<(ControlKind, String), usize> =
        std::collections::HashMap::new();
    let mut out = Vec::new();
    let mut found = b.controls()?;
    let deadline = Instant::now() + Duration::from_secs(3);
    while found.is_empty() && Instant::now() < deadline {
        std::thread::sleep(POLL);
        found = b.controls()?;
    }
    for (kind, e) in found {
        let label = name_of(&e);
        if label.is_empty() {
            continue;
        }
        let ordinal = *seen
            .entry((kind, label.clone()))
            .and_modify(|n| *n += 1)
            .or_insert(0);
        let sensitive =
            kind == ControlKind::TextField && (is_password(&e) || is_sensitive_label(&label));
        let value = if kind == ControlKind::TextField && !sensitive {
            value_of(&e)
        } else {
            None
        };
        out.push(ControlRef {
            kind,
            label,
            ordinal,
            page_address: page_address.clone(),
            sensitive,
            value,
        });
        if out.len() >= MAX_CONTROLS {
            break;
        }
    }
    Ok(out)
}

/// Presses an observed button (Invoke), after re-checking it is the same button on the same page.
pub fn press_button(exe: &Path, c: &ControlRef) -> Result<PressResult, String> {
    if c.kind != ControlKind::Button {
        return Err("That's a text box, not a button.".into());
    }
    let b = Browser::find(exe)?;
    let before = b.address();
    let target = b.find_control(c)?;
    let invoke =
        unsafe { target.GetCurrentPatternAs::<IUIAutomationInvokePattern>(UIA_InvokePatternId) }
            .map_err(|_| "That button can't be pressed from here.".to_string())?;
    unsafe { invoke.Invoke() }.map_err(|_| "Pressing that button didn't work.".to_string())?;
    std::thread::sleep(Duration::from_millis(1200));
    let after = Browser::find(exe).ok().and_then(|b| b.address());
    if after.is_some() && after != before {
        wait_until_loaded(exe);
    }
    Ok(PressResult {
        address_changed: after.is_some() && after != before,
        address: after,
    })
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

fn unicode(unit: u16, up: bool) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(0),
                wScan: unit,
                dwFlags: if up {
                    KEYEVENTF_UNICODE | KEYEVENTF_KEYUP
                } else {
                    KEYEVENTF_UNICODE
                },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

/// Keystrokes that select the box's existing text and replace it with `text`.
/// No Enter, Tab or other control keys are ever sent, so typing alone can't submit a form.
fn replace_text_keystrokes(text: &str) -> Vec<INPUT> {
    const VK_A: VIRTUAL_KEY = VIRTUAL_KEY(0x41);
    let mut inputs = vec![
        key(VK_CONTROL, false),
        key(VK_A, false),
        key(VK_A, true),
        key(VK_CONTROL, true),
    ];
    if text.is_empty() {
        // Restoring an empty box: delete the selection.
        inputs.push(key(VK_DELETE, false));
        inputs.push(key(VK_DELETE, true));
    }
    for unit in text.encode_utf16() {
        inputs.push(unicode(unit, false));
        inputs.push(unicode(unit, true));
    }
    inputs
}

/// Presses the browser's own Back button (in the toolbar, above the page; never a "Back"
/// link inside the page). Only if the browser still shows `expected_current`, so it never goes
/// back from a page the person has moved to since. Verified by the address returning to
/// `expected_previous`.
pub fn go_back(
    exe: &Path,
    expected_current: Option<&str>,
    expected_previous: Option<&str>,
) -> Result<NavigationResult, String> {
    let b = Browser::find(exe)?;
    let current = b.address();
    if current.as_deref() != expected_current {
        return Err(
            "You've moved to a different page since then, so I won't go back blindly.".into(),
        );
    }
    let page_top = unsafe { b.document()?.CurrentBoundingRectangle() }
        .map(|r| r.top)
        .unwrap_or(i32::MAX);
    let buttons = control_type(&b.a, UIA_ButtonControlTypeId)?;
    let all = unsafe { b.root.FindAll(TreeScope_Descendants, &buttons) }
        .map_err(|_| "I couldn't find the browser's Back button.".to_string())?;
    let back = (0..unsafe { all.Length() }.unwrap_or(0))
        .filter_map(|i| unsafe { all.GetElement(i) }.ok())
        .find(|e| {
            name_of(e) == "Back"
                && unsafe { e.CurrentBoundingRectangle() }
                    .map(|r| r.bottom <= page_top)
                    .unwrap_or(false)
                && unsafe { e.CurrentIsEnabled() }
                    .map(|x| x.as_bool())
                    .unwrap_or(false)
        })
        .ok_or_else(|| {
            "I couldn't find the browser's Back button (or there's no earlier page).".to_string()
        })?;
    let invoke =
        unsafe { back.GetCurrentPatternAs::<IUIAutomationInvokePattern>(UIA_InvokePatternId) }
            .map_err(|_| "The Back button can't be pressed from here.".to_string())?;
    unsafe { invoke.Invoke() }.map_err(|_| "Pressing Back didn't work.".to_string())?;

    let deadline = Instant::now() + NAVIGATION_TIMEOUT;
    let mut after = current.clone();
    while Instant::now() < deadline {
        std::thread::sleep(POLL);
        if let Ok(b) = Browser::find(exe) {
            after = b.address();
            if after.is_some() && after.as_deref() == expected_previous {
                wait_until_loaded(exe);
                return Ok(NavigationResult {
                    verified: true,
                    address: after,
                });
            }
        }
    }
    Ok(NavigationResult {
        verified: false,
        address: after,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ZoomDirection {
    In,
    Out,
    Reset,
}

/// The browser's own page zoom (Ctrl + Plus / Minus / 0). Sent only when the browser is
/// verifiably the foreground window, like typing.
pub fn zoom_page(exe: &Path, direction: ZoomDirection) -> Result<(), String> {
    const VK_0: VIRTUAL_KEY = VIRTUAL_KEY(0x30);
    let b = Browser::find(exe)?;
    unsafe {
        let _ = SetForegroundWindow(b.hwnd);
    }
    std::thread::sleep(Duration::from_millis(250));
    if unsafe { GetForegroundWindow() } != b.hwnd {
        return Err("I couldn't bring the browser to the front (another window may be in front), so I didn't change anything. Click on the browser window and ask me again.".into());
    }
    let vk = match direction {
        ZoomDirection::In => VK_OEM_PLUS,
        ZoomDirection::Out => VK_OEM_MINUS,
        ZoomDirection::Reset => VK_0,
    };
    let inputs = [
        key(VK_CONTROL, false),
        key(vk, false),
        key(vk, true),
        key(VK_CONTROL, true),
    ];
    let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
    if sent as usize != inputs.len() {
        return Err("Windows blocked the zoom keys.".into());
    }
    Ok(())
}

/// Types `text` into an observed text box, replacing what's there.
///
/// Safety: refuses sensitive fields; brings the browser to the front and focuses the box,
/// then types only if the browser really is the foreground window AND that box has keyboard
/// focus (otherwise keystrokes could land somewhere else). Reads the box back to verify.
pub fn type_into_field(exe: &Path, c: &ControlRef, text: &str) -> Result<TypeResult, String> {
    if c.kind != ControlKind::TextField {
        return Err("That's a button, not a text box.".into());
    }
    if c.sensitive {
        return Err("That box is for a password or payment details. For your safety I never type into those; please fill it in yourself.".into());
    }
    if text.chars().any(|ch| ch.is_control()) {
        return Err("The text can't contain line breaks or special keys.".into());
    }
    let b = Browser::find(exe)?;
    let field = b.find_control(c)?;
    if is_password(&field) {
        return Err("That box is for a password. For your safety I never type into those.".into());
    }
    unsafe {
        let _ = SetForegroundWindow(b.hwnd);
    }
    std::thread::sleep(Duration::from_millis(250));
    unsafe { field.SetFocus() }
        .map_err(|_| "I couldn't put the cursor in that box.".to_string())?;
    std::thread::sleep(Duration::from_millis(200));

    let browser_in_front = unsafe { GetForegroundWindow() } == b.hwnd;
    let field_focused = unsafe { field.CurrentHasKeyboardFocus() }
        .map(|f| f.as_bool())
        .unwrap_or(false);
    if !browser_in_front || !field_focused {
        return Err("I couldn't safely put the cursor in that box (another window may be in front), so I didn't type anything. Click on the browser window and ask me again.".into());
    }

    let inputs = replace_text_keystrokes(text);
    let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
    if sent as usize != inputs.len() {
        return Err(
            "Windows blocked the typing, so the box may be only partly filled. Please check it."
                .into(),
        );
    }
    std::thread::sleep(Duration::from_millis(250));
    let now = value_of(&field).unwrap_or_default();
    Ok(TypeResult {
        verified: now.trim() == text.trim(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn address_matching() {
        assert!(address_matches("example.com", "example.com"));
        assert!(address_matches(
            "https://www.example.com/path",
            "example.com"
        ));
        assert!(address_matches("bbc.co.uk/news", "bbc.co.uk"));
        assert!(address_matches("www.bbc.co.uk", "bbc.co.uk"));
        assert!(address_matches("news.bbc.co.uk", "bbc.co.uk"));
        assert!(!address_matches("example.com.evil.net", "example.com"));
        assert!(!address_matches("notexample.com", "example.com"));
        assert!(!address_matches("evil.net/?q=example.com", "example.com"));
    }

    #[test]
    fn sensitive_field_labels() {
        for l in [
            "Password",
            "Card number",
            "CVV",
            "Security code",
            "Expiry date",
            "PIN",
            "Sort code",
            "One-time code",
        ] {
            assert!(is_sensitive_label(l), "{l}");
        }
        for l in [
            "Search products",
            "Your name",
            "Delivery address",
            "Postcode",
            "Email",
            "Spinach",
            "Discard",
        ] {
            assert!(!is_sensitive_label(l), "{l}");
        }
    }

    #[test]
    fn typing_never_sends_enter_or_tab() {
        let inputs = replace_text_keystrokes("milk & eggs 🥚");
        // Ctrl+A first (4 events), then only Unicode character events.
        for i in &inputs[4..] {
            let ki = unsafe { i.Anonymous.ki };
            assert!(ki.dwFlags.contains(KEYEVENTF_UNICODE));
            assert_eq!(ki.wVk, VIRTUAL_KEY(0));
        }
        // Emoji is a surrogate pair: 2 units, down+up each.
        assert_eq!(
            inputs.len(),
            4 + 2 * "milk & eggs 🥚".encode_utf16().count()
        );
    }

    #[test]
    fn article_text_drops_menus_and_keeps_prose() {
        let page = "Home\nNews\nSport\nSign in\nThe council met on Tuesday to discuss the new library opening hours.\nResidents welcomed the longer weekend hours.\nShare\nMore stories about the town centre and the library were published this week.\nNext";
        let a = article_text(page);
        assert!(a.starts_with("The council met"));
        assert!(a.contains("Residents welcomed"));
        assert!(!a.contains("Sign in"));
        assert!(!a.lines().any(|l| l == "Share" || l == "Next"));
    }

    #[test]
    fn article_text_falls_back_to_everything_when_little_prose() {
        let page = "Milk\nBread\nEggs";
        assert_eq!(article_text(page), page);
    }

    #[test]
    fn tidy_collapses_whitespace_and_blank_lines() {
        assert_eq!(tidy("  Hello   world \n\n\t\nBye  "), "Hello world\nBye\n");
    }

    fn fixture_url(name: &str) -> String {
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name);
        format!("file:///{}", p.to_string_lossy().replace('\\', "/"))
    }

    /// Live check against the real default browser using the local test page (file://,
    /// opened directly here; the model-facing tool only allows https).
    /// VISIBLE SIDE EFFECT: opens tabs. Run:
    /// cargo test --manifest-path src-tauri/Cargo.toml live_browser -- --ignored --nocapture --test-threads=1
    #[test]
    #[ignore = "opens the local test page in the real browser"]
    fn live_browser_local_test_page() {
        let exe = super::super::win::default_browser_exe().expect("default browser");
        Command::new(&exe)
            .arg(fixture_url("browser-test.html"))
            .spawn()
            .unwrap();
        let deadline = Instant::now() + NAVIGATION_TIMEOUT;
        let mut page = None;
        while Instant::now() < deadline && page.is_none() {
            std::thread::sleep(POLL);
            page = read_page(&exe)
                .ok()
                .filter(|p| p.title.contains("Assistant Test Page"));
        }
        let page = page.expect("test page should load and be readable");
        println!(
            "title: {}
address: {:?}
text:
{}",
            page.title, page.address, page.text
        );
        assert!(page.text.contains("marmalade"));

        let links = list_links(&exe).expect("links");
        println!("links: {links:?}");
        assert!(links.iter().any(|l| l.text == "Go to the second page"));
        assert!(links.iter().any(|l| l.text == "Delete my account"));

        println!("scroll down: {:?}", scroll_page(&exe, true));
        println!("scroll up: {:?}", scroll_page(&exe, false));

        let second = links
            .iter()
            .find(|l| l.text == "Go to the second page")
            .unwrap();
        let nav = follow_link(&exe, second).expect("follow");
        println!("follow: {nav:?}");
        assert!(
            nav.verified,
            "address should change after following the link"
        );
        let page2 = read_page(&exe).expect("read page 2");
        assert!(page2.text.contains("You followed the link correctly"));

        // A stale link (observed on page 1) must not be clicked on page 2.
        let stale = follow_link(&exe, second);
        println!("stale follow: {stale:?}");
        assert!(stale.is_err());

        // Undo: the toolbar Back button returns to page one (and refuses if we're elsewhere).
        let wrong = go_back(
            &exe,
            Some("https://not-this-page.example/"),
            second.page_address.as_deref(),
        );
        assert!(wrong.is_err(), "must not go back from an unexpected page");
        let back =
            go_back(&exe, nav.address.as_deref(), second.page_address.as_deref()).expect("back");
        println!("back: {back:?}");
        assert!(back.verified, "should be back on the first page");
    }

    /// Live: the pretend shop. Lists controls, types into the search box, presses Search and
    /// an Add button, and checks the card-number box is refused. Opens a browser tab and
    /// briefly brings the browser to the front to type. Run:
    /// cargo test --manifest-path src-tauri/Cargo.toml live_shop -- --ignored --nocapture --test-threads=1
    #[test]
    #[ignore = "opens the pretend shop in the real browser and types into it"]
    fn live_shop_controls() {
        let exe = super::super::win::default_browser_exe().expect("default browser");
        Command::new(&exe)
            .arg(fixture_url("shop-test.html"))
            .spawn()
            .unwrap();
        let deadline = Instant::now() + NAVIGATION_TIMEOUT;
        let mut ready = false;
        while Instant::now() < deadline && !ready {
            std::thread::sleep(POLL);
            ready = read_page(&exe).is_ok_and(|p| p.title.contains("Pretend Grocery Shop"));
        }
        assert!(ready, "shop page should load");
        let controls = list_controls(&exe).expect("controls");
        for c in &controls {
            println!(
                "{:?} {:?} sensitive={} value={:?}",
                c.kind, c.label, c.sensitive, c.value
            );
        }
        let search = controls
            .iter()
            .find(|c| c.kind == ControlKind::TextField && c.label.contains("Search"))
            .expect("search box");
        match type_into_field(&exe, search, "milk") {
            Ok(r) => {
                println!("typed: {r:?}");
                assert!(r.verified);
                let button = controls
                    .iter()
                    .find(|c| c.kind == ControlKind::Button && c.label == "Search")
                    .unwrap();
                println!("press search: {:?}", press_button(&exe, button));
                std::thread::sleep(Duration::from_millis(500));
                let page = read_page(&exe).unwrap();
                assert!(page.text.contains("Results for"), "{}", page.text);
            }
            Err(e) => println!(
                "SKIPPED typing (expected if this process can't bring the browser to the front): {e}"
            ),
        }
        let add = controls
            .iter()
            .find(|c| c.label == "Add bread to basket")
            .unwrap();
        // Page address unchanged by the search, so the observation is still valid.
        println!("press add: {:?}", press_button(&exe, add));
        std::thread::sleep(Duration::from_millis(500));
        assert!(read_page(&exe).unwrap().text.contains("Basket: bread"));
        let checkout = list_controls(&exe)
            .unwrap()
            .into_iter()
            .find(|c| c.label == "Checkout")
            .unwrap();
        press_button(&exe, &checkout).unwrap();
        std::thread::sleep(Duration::from_millis(500));
        let card = list_controls(&exe)
            .unwrap()
            .into_iter()
            .find(|c| c.label == "Card number")
            .expect("card box");
        assert!(card.sensitive);
        let refused = type_into_field(&exe, &card, "4111111111111111");
        println!("card refused: {refused:?}");
        assert!(refused.is_err());
    }
}
