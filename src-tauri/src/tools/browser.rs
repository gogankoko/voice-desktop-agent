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

use windows::Win32::Foundation::{HWND, LPARAM};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
};
use windows::Win32::System::Variant::{VARIANT, VT_I4};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationCondition, IUIAutomationElement,
    IUIAutomationInvokePattern, IUIAutomationScrollPattern, IUIAutomationTextPattern,
    IUIAutomationValuePattern, ScrollAmount_LargeDecrement, ScrollAmount_LargeIncrement,
    ScrollAmount_NoAmount, TreeScope_Descendants, UIA_CONTROLTYPE_ID, UIA_ControlTypePropertyId,
    UIA_DocumentControlTypeId, UIA_EditControlTypeId, UIA_HyperlinkControlTypeId,
    UIA_InvokePatternId, UIA_ScrollPatternId, UIA_TextPatternId, UIA_ValuePatternId,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindowTextLengthW, GetWindowThreadProcessId, IsWindowVisible,
};
use windows::core::BOOL;

use super::web_url::SafeUrl;
use super::win::process_image_path;

const MAX_PAGE_CHARS: usize = 6000;
const MAX_LINKS: usize = 40;
const NAVIGATION_TIMEOUT: Duration = Duration::from_secs(10);
const POLL: Duration = Duration::from_millis(300);

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
        eprintln!("UI Automation unavailable: {e}");
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

/// Visible top-level windows in z-order (front first).
fn top_level_windows() -> Vec<HWND> {
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
    root: IUIAutomationElement,
}

const NO_BROWSER: &str = "I can't find your web browser window. Ask me to open a website first.";

impl Browser {
    fn find(exe: &Path) -> Result<Self, String> {
        let a = automation()?;
        let hwnd = browser_window(exe).ok_or_else(|| NO_BROWSER.to_string())?;
        let root = unsafe { a.ElementFromHandle(hwnd) }.map_err(|_| NO_BROWSER.to_string())?;
        Ok(Self { a, root })
    }

    /// The web page itself (Chromium-based browsers and Firefox expose it as a Document).
    fn document(&self) -> Result<IUIAutomationElement, String> {
        let cond = control_type(&self.a, UIA_DocumentControlTypeId)?;
        unsafe { self.root.FindFirst(TreeScope_Descendants, &cond) }
            .map_err(|_| "I can't see a web page in the browser right now.".to_string())
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

/// Opens `url` in the browser (a new tab), then waits for the address bar to show the site.
pub fn open_website(exe: &Path, url: &SafeUrl) -> Result<NavigationResult, String> {
    // `url.url` always starts with "https://", so it can't be read as a command-line flag.
    Command::new(exe).arg(&url.url).spawn().map_err(|e| {
        eprintln!("open_website: launch failed: {e}");
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
    let b = Browser::find(exe)?;
    let doc = b.document()?;
    let title = name_of(&doc);
    let text = unsafe { doc.GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId) }
        .and_then(|p| unsafe { p.DocumentRange() })
        .and_then(|r| unsafe { r.GetText(200_000) })
        .map(|t| tidy(&t.to_string()))
        .map_err(|_| "I couldn't read the text on this page.".to_string())?;
    let truncated = text.chars().count() > MAX_PAGE_CHARS;
    let text = if truncated {
        text.chars().take(MAX_PAGE_CHARS).collect()
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
    }
}
