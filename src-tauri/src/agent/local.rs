//! Built-in commands: everyday requests the app understands by itself, instantly, for free
//! and without the internet ("open YouTube", "pause", "next song", "set a timer for 10
//! minutes", "how much battery do I have?").
//!
//! Only short, fixed-shape requests match; anything else (e.g. "play Yeat on Spotify",
//! "open YouTube and search for...") goes to the AI. A match becomes ordinary tool calls
//! played through the same request loop as the AI (`LocalModel`), so every built-in command
//! is validated, risk-checked, reported and undoable exactly like an AI-chosen one.

use std::sync::Mutex;

use serde_json::{Value, json};

use super::gemini::{ChatModel, ModelError, ModelTurn, ProposedCall};
use crate::tools::apps::{InstalledApp, MediaAction};
use crate::tools::window_ops::WindowAction;

const CALL_ID: &str = "local-1";

/// Popular websites by what people call them. (The app wins if it's installed with that
/// exact name, e.g. Spotify.)
const SITES: &[(&[&str], &str, &str)] = &[
    (
        &["youtube", "you tube"],
        "YouTube",
        "https://www.youtube.com",
    ),
    (
        &["google", "google search"],
        "Google",
        "https://www.google.com",
    ),
    (
        &["gmail", "google mail", "my email", "my emails", "email"],
        "Gmail",
        "https://mail.google.com",
    ),
    (
        &["google maps", "maps"],
        "Google Maps",
        "https://www.google.com/maps",
    ),
    (
        &["google translate", "translate"],
        "Google Translate",
        "https://translate.google.com",
    ),
    (
        &["google drive", "drive"],
        "Google Drive",
        "https://drive.google.com",
    ),
    (
        &["google news", "news"],
        "Google News",
        "https://news.google.com",
    ),
    (&["facebook"], "Facebook", "https://www.facebook.com"),
    (&["instagram"], "Instagram", "https://www.instagram.com"),
    (&["twitter", "x"], "X", "https://x.com"),
    (&["reddit"], "Reddit", "https://www.reddit.com"),
    (&["wikipedia"], "Wikipedia", "https://www.wikipedia.org"),
    (&["amazon"], "Amazon", "https://www.amazon.com"),
    (&["ebay"], "eBay", "https://www.ebay.com"),
    (&["netflix"], "Netflix", "https://www.netflix.com"),
    (&["tiktok", "tik tok"], "TikTok", "https://www.tiktok.com"),
    (
        &["linkedin", "linked in"],
        "LinkedIn",
        "https://www.linkedin.com",
    ),
    (
        &["whatsapp", "whatsapp web", "whats app"],
        "WhatsApp",
        "https://web.whatsapp.com",
    ),
    (
        &["outlook", "hotmail"],
        "Outlook",
        "https://outlook.live.com",
    ),
    (
        &["spotify", "spotify web"],
        "Spotify",
        "https://open.spotify.com",
    ),
    (
        &["soundcloud", "sound cloud"],
        "SoundCloud",
        "https://soundcloud.com",
    ),
    (&["twitch"], "Twitch", "https://www.twitch.tv"),
    (&["pinterest"], "Pinterest", "https://www.pinterest.com"),
    (&["discord"], "Discord", "https://discord.com/app"),
    (&["github", "git hub"], "GitHub", "https://github.com"),
    (&["chatgpt", "chat gpt"], "ChatGPT", "https://chatgpt.com"),
    (&["claude"], "Claude", "https://claude.ai"),
    (&["bbc", "bbc news"], "BBC", "https://www.bbc.com"),
    (&["cnn"], "CNN", "https://www.cnn.com"),
    (&["espn"], "ESPN", "https://www.espn.com"),
    (
        &["the weather", "weather"],
        "the weather",
        "https://weather.com",
    ),
    (&["imdb"], "IMDb", "https://www.imdb.com"),
    (&["duolingo"], "Duolingo", "https://www.duolingo.com"),
    (&["yahoo"], "Yahoo", "https://www.yahoo.com"),
    (&["bing"], "Bing", "https://www.bing.com"),
    (&["canva"], "Canva", "https://www.canva.com"),
];

/// Apps with their own fixed entry (open_application).
const BUILT_IN_APPS: &[(&[&str], &str, &str)] = &[
    (
        &[
            "browser",
            "web browser",
            "internet",
            "the internet",
            "my browser",
        ],
        "default_browser",
        "your browser",
    ),
    (&["notepad", "note pad"], "notepad", "Notepad"),
    (&["calculator", "calc"], "calculator", "Calculator"),
    (&["paint", "ms paint", "microsoft paint"], "paint", "Paint"),
    (
        &[
            "file explorer",
            "explorer",
            "files",
            "my files",
            "file manager",
        ],
        "file_explorer",
        "File Explorer",
    ),
    (&["settings", "windows settings"], "settings", "Settings"),
];

const OPEN_VERBS: &[&str] = &[
    "open up",
    "open",
    "launch",
    "start up",
    "start",
    "bring up",
    "pull up",
    "go to",
    "take me to",
    "show me",
    "load",
    "visit",
    "get me",
    "run",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusTopic {
    Battery,
    Internet,
    Disk,
}

/// How to word the answer once the command has run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reply {
    Opening(String),
    Media(MediaAction),
    Timer { minutes: u32 },
    Reminder { text: String },
    ListReminders,
    Status(StatusTopic),
    Window(WindowAction),
    Magnifier { on: bool },
    Undo,
    Time,
    Date,
}

/// One built-in command: at most one tool call, and how to answer.
#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    pub call: Option<(&'static str, Value)>,
    pub reply: Reply,
}

/// Lower case; apostrophes dropped ("what's" -> "whats"); other punctuation becomes spaces,
/// except dots inside words (domains like bbc.com).
pub fn normalise(text: &str) -> String {
    let chars: Vec<char> = text.to_lowercase().chars().collect();
    let mut out = String::new();
    for (i, &c) in chars.iter().enumerate() {
        if c == '\'' || c == '\u{2019}' {
            continue;
        }
        let inner_dot = c == '.'
            && i > 0
            && i + 1 < chars.len()
            && chars[i - 1].is_alphanumeric()
            && chars[i + 1].is_alphanumeric();
        out.push(if c.is_alphanumeric() || inner_dot {
            c
        } else {
            ' '
        });
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Removes politeness around the command: "hey, could you please ... for me now, thanks".
fn strip_politeness(s: &str) -> String {
    const PREFIXES: &[&str] = &[
        "hey assistant ",
        "hey ",
        "hi ",
        "ok ",
        "okay ",
        "assistant ",
        "please ",
        "can you please ",
        "could you please ",
        "would you please ",
        "can you ",
        "could you ",
        "would you ",
        "will you ",
        "i want you to ",
        "i want to ",
        "i would like to ",
        "id like to ",
        "lets ",
        "go ahead and ",
        "just ",
        "quickly ",
    ];
    const SUFFIXES: &[&str] = &[
        " please",
        " for me",
        " right now",
        " now",
        " thanks",
        " thank you",
        " real quick",
        " quickly",
    ];
    let mut s = format!("{s} ").trim_start().to_string();
    s = s.trim_end().to_string();
    loop {
        let before = s.clone();
        for p in PREFIXES {
            if let Some(rest) = s.strip_prefix(p) {
                s = rest.to_string();
            }
        }
        for x in SUFFIXES {
            if let Some(rest) = s.strip_suffix(x) {
                s = rest.to_string();
            }
        }
        if s == before {
            return s;
        }
    }
}

fn is_one_of(s: &str, options: &[&str]) -> bool {
    options.contains(&s)
}

/// "5", "five", "twenty five", "a", "an" -> number.
fn number(words: &[&str]) -> Option<u32> {
    let unit = |w: &str| -> Option<u32> {
        Some(match w {
            "a" | "an" | "one" => 1,
            "two" => 2,
            "three" => 3,
            "four" => 4,
            "five" => 5,
            "six" => 6,
            "seven" => 7,
            "eight" => 8,
            "nine" => 9,
            "ten" => 10,
            "eleven" => 11,
            "twelve" => 12,
            "thirteen" => 13,
            "fourteen" => 14,
            "fifteen" => 15,
            "sixteen" => 16,
            "seventeen" => 17,
            "eighteen" => 18,
            "nineteen" => 19,
            _ => return w.parse().ok().filter(|n| *n <= 1440),
        })
    };
    let tens = |w: &str| -> Option<u32> {
        Some(match w {
            "twenty" => 20,
            "thirty" => 30,
            "forty" => 40,
            "fifty" => 50,
            "sixty" => 60,
            "seventy" => 70,
            "eighty" => 80,
            "ninety" => 90,
            _ => return None,
        })
    };
    match words {
        [w] => unit(w).or_else(|| tens(w)),
        [t, u] => Some(tens(t)? + unit(u).filter(|n| *n < 10)?),
        _ => None,
    }
}

/// "10 minutes", "an hour", "two hours", "half an hour", "45 min" -> minutes (1..=1440).
fn duration(s: &str) -> Option<u32> {
    if s == "half an hour" || s == "half hour" {
        return Some(30);
    }
    let words: Vec<&str> = s.split(' ').collect();
    let (unit, count) = words.split_last()?;
    let per = match *unit {
        "minute" | "minutes" | "min" | "mins" => 1,
        "hour" | "hours" | "hr" | "hrs" => 60,
        _ => return None,
    };
    let minutes = number(count)? * per;
    (1..=crate::reminders::MAX_MINUTES)
        .contains(&minutes)
        .then_some(minutes)
}

/// "10 minute" / "an hour" in "set a 10 minute timer".
fn timer_length(s: &str) -> Option<u32> {
    duration(s).or_else(|| duration(&format!("{s}s")))
}

pub fn plain_duration(minutes: u32) -> String {
    let (h, m) = (minutes / 60, minutes % 60);
    let part = |n: u32, unit: &str| {
        if n == 1 {
            format!("1 {unit}")
        } else {
            format!("{n} {unit}s")
        }
    };
    match (h, m) {
        (0, m) => part(m, "minute"),
        (h, 0) => part(h, "hour"),
        (h, m) => format!("{} and {}", part(h, "hour"), part(m, "minute")),
    }
}

fn media(s: &str) -> Option<MediaAction> {
    const THING: &[&str] = &[
        "",
        " it",
        " the music",
        " the song",
        " the track",
        " the video",
        " music",
        " spotify",
        " the playback",
        " playback",
        " this",
        " this song",
    ];
    let with = |verbs: &[&str]| -> bool {
        verbs
            .iter()
            .any(|v| THING.iter().any(|t| s == format!("{v}{t}")))
    };
    if is_one_of(
        s,
        &[
            "continue the music",
            "continue playing",
            "keep playing",
            "continue the song",
        ],
    ) {
        return Some(MediaAction::PlayPause);
    }
    if with(&["pause", "resume", "unpause", "stop", "play"]) {
        // "stop" alone is the app's own Stop; "play" alone resumes.
        if s == "stop" {
            return None;
        }
        return Some(MediaAction::PlayPause);
    }
    if is_one_of(
        s,
        &[
            "next",
            "next song",
            "next track",
            "next one",
            "skip",
            "skip it",
            "skip this",
            "skip this song",
            "skip the song",
            "skip song",
            "skip track",
            "play the next song",
            "play next song",
            "go to the next song",
        ],
    ) {
        return Some(MediaAction::NextTrack);
    }
    if is_one_of(
        s,
        &[
            "previous",
            "previous song",
            "previous track",
            "last song",
            "go back a song",
            "play the previous song",
            "play the last song",
            "back a song",
            "go to the previous song",
        ],
    ) {
        return Some(MediaAction::PreviousTrack);
    }
    if is_one_of(
        s,
        &[
            "volume up",
            "turn it up",
            "turn up the volume",
            "turn the volume up",
            "louder",
            "make it louder",
            "increase the volume",
            "raise the volume",
            "turn up the music",
            "turn the music up",
            "a bit louder",
            "little louder",
            "a little louder",
        ],
    ) {
        return Some(MediaAction::VolumeUp);
    }
    if is_one_of(
        s,
        &[
            "volume down",
            "turn it down",
            "turn down the volume",
            "turn the volume down",
            "quieter",
            "make it quieter",
            "lower the volume",
            "decrease the volume",
            "turn down the music",
            "turn the music down",
            "a bit quieter",
            "a little quieter",
            "softer",
        ],
    ) {
        return Some(MediaAction::VolumeDown);
    }
    if is_one_of(
        s,
        &[
            "mute",
            "unmute",
            "mute it",
            "unmute it",
            "mute the sound",
            "mute the volume",
            "mute the music",
            "mute audio",
            "mute the audio",
            "mute sound",
            "unmute the sound",
        ],
    ) {
        return Some(MediaAction::Mute);
    }
    None
}

fn reminder_plan(s: &str) -> Option<Plan> {
    // Timers.
    for prefix in [
        "set a timer for ",
        "set timer for ",
        "timer for ",
        "start a timer for ",
        "set an alarm for ",
    ] {
        if let Some(rest) = s.strip_prefix(prefix) {
            let minutes = duration(rest)?;
            return Some(timer(minutes));
        }
    }
    for (prefix, suffix) in [
        ("set a ", " timer"),
        ("set an ", " timer"),
        ("start a ", " timer"),
        ("", " timer"),
    ] {
        if let Some(mid) = s.strip_prefix(prefix).and_then(|r| r.strip_suffix(suffix)) {
            if let Some(minutes) = timer_length(mid) {
                return Some(timer(minutes));
            }
        }
    }
    // "remind me in 20 minutes to X" / "remind me to X in 20 minutes".
    let rest = s.strip_prefix("remind me ")?;
    let (minutes, text) = if let Some(r) = rest.strip_prefix("in ") {
        let (when, what) = r.split_once(" to ")?;
        (duration(when)?, what)
    } else {
        let r = rest.strip_prefix("to ")?;
        let (what, when) = r.rsplit_once(" in ")?;
        (duration(when)?, what)
    };
    let text = text.trim();
    if text.is_empty() || text.chars().count() > crate::reminders::MAX_TEXT_CHARS {
        return None;
    }
    Some(Plan {
        call: Some((
            "set_reminder",
            json!({ "text": text, "in_minutes": minutes }),
        )),
        reply: Reply::Reminder {
            text: text.to_string(),
        },
    })
}

fn timer(minutes: u32) -> Plan {
    Plan {
        call: Some((
            "set_reminder",
            json!({
                "text": format!("Your timer for {} is done", plain_duration(minutes)),
                "in_minutes": minutes
            }),
        )),
        reply: Reply::Timer { minutes },
    }
}

/// The app the person named, if one in the Start menu has exactly that name (ignoring a
/// leading "Microsoft" / "Google", so "teams" finds "Microsoft Teams").
pub fn exact_app<'a>(apps: &'a [InstalledApp], name: &str) -> Option<&'a InstalledApp> {
    let n = normalise(name);
    apps.iter().find(|a| {
        let an = normalise(&a.name);
        an == n
            || an.strip_prefix("microsoft ") == Some(n.as_str())
            || an.strip_prefix("google ") == Some(n.as_str())
    })
}

/// What follows "open" in a short "open X" request, if the request is one. Used to decide
/// whether the Start menu needs to be read at all.
pub fn open_target(utterance: &str) -> Option<String> {
    let s = strip_politeness(&normalise(utterance));
    let rest = OPEN_VERBS
        .iter()
        .find_map(|v| s.strip_prefix(&format!("{v} ")))?;
    let mut t = rest.trim().to_string();
    for article in ["the ", "my ", "a "] {
        if let Some(r) = t.strip_prefix(article) {
            // "the internet", "my files", "my email" are names in the tables.
            if !BUILT_IN_APPS
                .iter()
                .any(|(names, _, _)| names.contains(&t.as_str()))
                && !SITES
                    .iter()
                    .any(|(names, _, _)| names.contains(&t.as_str()))
            {
                t = r.to_string();
            }
        }
    }
    for suffix in [
        " app",
        " application",
        " website",
        " site",
        " web site",
        " page",
        " program",
    ] {
        if let Some(r) = t.strip_suffix(suffix) {
            t = r.to_string();
        }
    }
    let words = t.split(' ').count();
    (!t.is_empty() && words <= 4).then_some(t)
}

fn open_plan(target: &str, apps: &[InstalledApp], bare: bool) -> Option<Plan> {
    if let Some((_, id, name)) = BUILT_IN_APPS
        .iter()
        .find(|(names, _, _)| names.contains(&target))
    {
        return Some(Plan {
            call: Some(("open_application", json!({ "app": id }))),
            reply: Reply::Opening(name.to_string()),
        });
    }
    if !bare {
        if let Some(app) = exact_app(apps, target) {
            return Some(Plan {
                call: Some(("open_installed_app", json!({ "name": app.name }))),
                reply: Reply::Opening(app.name.clone()),
            });
        }
    }
    if let Some((_, name, url)) = SITES.iter().find(|(names, _, _)| names.contains(&target)) {
        return Some(Plan {
            call: Some(("open_website", json!({ "url": url }))),
            reply: Reply::Opening(name.to_string()),
        });
    }
    // "open bbc.co.uk": a plain web address (checked like any other by open_website).
    if !bare
        && !target.contains(' ')
        && target.contains('.')
        && target
            .rsplit('.')
            .next()
            .is_some_and(|tld| tld.len() >= 2 && tld.chars().all(|c| c.is_ascii_alphabetic()))
    {
        return Some(Plan {
            call: Some((
                "open_website",
                json!({ "url": format!("https://{target}") }),
            )),
            reply: Reply::Opening(target.to_string()),
        });
    }
    None
}

/// The built-in command for `utterance`, if it is one. `apps` is the Start menu (may be
/// empty if the request isn't an "open X" request).
pub fn plan(utterance: &str, apps: &[InstalledApp]) -> Option<Plan> {
    let s = strip_politeness(&normalise(utterance));
    if s.is_empty() || s.split(' ').count() > 14 {
        return None;
    }
    // Several things at once ("open YouTube and search for...") is for the AI.
    if s.contains(" and ") || s.contains(" then ") {
        return None;
    }
    if let Some(a) = media(&s) {
        return Some(Plan {
            call: Some(("media_control", json!({ "action": a }))),
            reply: Reply::Media(a),
        });
    }
    if let Some(p) = reminder_plan(&s) {
        return Some(p);
    }
    if is_one_of(
        &s,
        &[
            "what reminders do i have",
            "what are my reminders",
            "list my reminders",
            "show my reminders",
            "my reminders",
            "do i have any reminders",
            "any reminders",
            "what timers do i have",
            "do i have any timers",
            "what timers are set",
            "what reminders are set",
            "check my reminders",
            "check my timers",
        ],
    ) {
        return Some(Plan {
            call: Some(("list_reminders", json!({}))),
            reply: Reply::ListReminders,
        });
    }
    let status = |t| Plan {
        call: Some(("computer_status", json!({}))),
        reply: Reply::Status(t),
    };
    if is_one_of(
        &s,
        &[
            "how much battery do i have",
            "how much battery is left",
            "how much battery",
            "how much battery have i got",
            "whats my battery",
            "whats my battery level",
            "what is my battery level",
            "whats the battery",
            "whats the battery level",
            "battery",
            "battery level",
            "battery status",
            "is my laptop charging",
            "is my computer charging",
            "is it charging",
            "am i charging",
            "check my battery",
            "check the battery",
            "how is my battery",
        ],
    ) {
        return Some(status(StatusTopic::Battery));
    }
    if is_one_of(
        &s,
        &[
            "am i online",
            "am i connected",
            "am i connected to the internet",
            "is the internet working",
            "is my internet working",
            "do i have internet",
            "am i connected to wifi",
            "am i on the internet",
            "is my wifi working",
            "check my internet",
            "check the internet",
        ],
    ) {
        return Some(status(StatusTopic::Internet));
    }
    if is_one_of(
        &s,
        &[
            "how much space do i have",
            "how much space is left",
            "how much disk space",
            "how much disk space do i have",
            "how much storage do i have",
            "how much storage is left",
            "is my computer full",
            "is my disk full",
            "check my storage",
            "how much free space",
        ],
    ) {
        return Some(status(StatusTopic::Disk));
    }
    let window = |a: WindowAction| Plan {
        call: Some(("manage_window", json!({ "action": a }))),
        reply: Reply::Window(a),
    };
    const IT: &[&str] = &["", " it", " this", " the window", " this window", " that"];
    let with_it = |verbs: &[&str]| {
        verbs
            .iter()
            .any(|v| IT.iter().any(|t| s == format!("{v}{t}")))
    };
    if with_it(&["minimize", "minimise", "hide"]) {
        return Some(window(WindowAction::Minimize));
    }
    if with_it(&["maximize", "maximise"])
        || is_one_of(
            &s,
            &[
                "full screen",
                "make it full screen",
                "make this full screen",
                "make the window full screen",
                "go full screen",
            ],
        )
    {
        return Some(window(WindowAction::Maximize));
    }
    if with_it(&["restore"]) {
        return Some(window(WindowAction::Restore));
    }
    if is_one_of(
        &s,
        &[
            "turn on the magnifier",
            "turn on magnifier",
            "magnifier on",
            "start the magnifier",
            "open the magnifier",
            "open magnifier",
            "make everything bigger",
            "make the screen bigger",
            "zoom in on the screen",
        ],
    ) {
        return Some(Plan {
            call: Some(("screen_magnifier", json!({ "action": "zoom_in" }))),
            reply: Reply::Magnifier { on: true },
        });
    }
    if is_one_of(
        &s,
        &[
            "turn off the magnifier",
            "turn off magnifier",
            "magnifier off",
            "stop the magnifier",
            "close the magnifier",
            "close magnifier",
        ],
    ) {
        return Some(Plan {
            call: Some(("screen_magnifier", json!({ "action": "turn_off" }))),
            reply: Reply::Magnifier { on: false },
        });
    }
    if is_one_of(
        &s,
        &[
            "undo",
            "undo that",
            "undo it",
            "undo the last thing",
            "undo the last action",
        ],
    ) {
        return Some(Plan {
            call: Some(("undo_last_action", json!({}))),
            reply: Reply::Undo,
        });
    }
    if is_one_of(
        &s,
        &[
            "what time is it",
            "whats the time",
            "what is the time",
            "tell me the time",
            "time",
            "the time",
            "what time is it now",
        ],
    ) {
        return Some(Plan {
            call: None,
            reply: Reply::Time,
        });
    }
    if is_one_of(
        &s,
        &[
            "whats the date",
            "what is the date",
            "whats todays date",
            "what is todays date",
            "what day is it",
            "what day is it today",
            "whats the date today",
            "todays date",
            "what is the date today",
            "the date",
        ],
    ) {
        return Some(Plan {
            call: None,
            reply: Reply::Date,
        });
    }
    if let Some(target) = open_target(utterance) {
        return open_plan(&target, apps, false);
    }
    // A bare site or app name ("YouTube", "Notepad"); not single letters misheard from noise.
    if s.len() < 3 {
        return None;
    }
    open_plan(&s, apps, true)
}

/// The local clock, for "what time is it".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Clock {
    pub year: u16,
    pub month: u16,
    pub day_of_week: u16,
    pub day: u16,
    pub hour: u16,
    pub minute: u16,
}

fn say_time(c: Clock) -> String {
    let (h12, half) = match c.hour {
        0 => (12, "AM"),
        1..=11 => (c.hour, "AM"),
        12 => (12, "PM"),
        _ => (c.hour - 12, "PM"),
    };
    format!("It's {h12}:{:02} {half}.", c.minute)
}

fn say_date(c: Clock) -> String {
    const DAYS: [&str; 7] = [
        "Sunday",
        "Monday",
        "Tuesday",
        "Wednesday",
        "Thursday",
        "Friday",
        "Saturday",
    ];
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
    format!(
        "Today is {}, {} {} {}.",
        DAYS.get(c.day_of_week as usize).unwrap_or(&""),
        c.day,
        MONTHS
            .get((c.month as usize).wrapping_sub(1))
            .unwrap_or(&""),
        c.year
    )
}

/// The spoken answer, from the plan and the tool's result (None: no tool was needed).
pub fn reply_text(reply: &Reply, result: Option<&Value>, clock: Clock) -> String {
    let r = result.cloned().unwrap_or(Value::Null);
    match reply {
        Reply::Opening(name) => format!("Opening {name}."),
        Reply::Media(a) => match a {
            MediaAction::PlayPause => "Okay.".into(),
            MediaAction::NextTrack => "Skipping to the next one.".into(),
            MediaAction::PreviousTrack => "Going back one.".into(),
            MediaAction::VolumeUp => "Turning it up.".into(),
            MediaAction::VolumeDown => "Turning it down.".into(),
            MediaAction::Mute => "Okay, I've pressed mute.".into(),
        },
        Reply::Timer { minutes } => format!(
            "Timer set for {}. It'll go off at {}. Keep this app open for it to ring.",
            plain_duration(*minutes),
            r["due_at_local_time"].as_str().unwrap_or("the right time")
        ),
        Reply::Reminder { text } => format!(
            "Okay, I'll remind you at {} to {text}. Keep this app open for it to go off.",
            r["due_at_local_time"].as_str().unwrap_or("the right time")
        ),
        Reply::ListReminders => {
            let list = r["reminders_soonest_first"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            if list.is_empty() {
                "You don't have any reminders or timers set.".into()
            } else {
                let items: Vec<String> = list
                    .iter()
                    .map(|x| {
                        format!(
                            "{} at {}",
                            x["text"].as_str().unwrap_or("a reminder"),
                            x["due_at_local_time"].as_str().unwrap_or("?")
                        )
                    })
                    .collect();
                format!(
                    "You have {} set: {}.",
                    if list.len() == 1 {
                        "one".to_string()
                    } else {
                        list.len().to_string()
                    },
                    items.join("; ")
                )
            }
        }
        Reply::Status(StatusTopic::Battery) => {
            if r["has_battery"] == false {
                "This computer doesn't have a battery, so it runs on mains power.".into()
            } else if let Some(p) = r["battery_percent"].as_u64() {
                match r["plugged_in"].as_bool() {
                    Some(true) => format!("Your battery is at {p} percent and plugged in."),
                    Some(false) => format!("Your battery is at {p} percent and not plugged in."),
                    None => format!("Your battery is at {p} percent."),
                }
            } else {
                "I couldn't read the battery level just now.".into()
            }
        }
        Reply::Status(StatusTopic::Internet) => match r["internet"].as_bool() {
            Some(true) => "Yes, you're connected to the internet.".into(),
            Some(false) => "You don't seem to be connected to the internet right now.".into(),
            None => "I couldn't tell whether you're connected to the internet.".into(),
        },
        Reply::Status(StatusTopic::Disk) => {
            match (r["disk_free_gb"].as_f64(), r["disk_total_gb"].as_f64()) {
                (Some(free), Some(total)) => format!(
                    "You have {free:.0} gigabytes free out of {total:.0} on your main drive."
                ),
                _ => "I couldn't check the disk space just now.".into(),
            }
        }
        Reply::Window(a) => {
            if r["done"] == true {
                match a {
                    WindowAction::Minimize => "Minimized.".into(),
                    WindowAction::Maximize => "It's full size now.".into(),
                    WindowAction::Restore => "Restored.".into(),
                    _ => "Done.".into(),
                }
            } else {
                "I tried, but couldn't confirm it worked.".into()
            }
        }
        Reply::Magnifier { on: true } => {
            "The screen is bigger now. Say \"turn off the magnifier\" to go back.".into()
        }
        Reply::Magnifier { on: false } => "Okay, the magnifier is off.".into(),
        Reply::Undo => {
            if r["undone"] == false {
                r["reason"]
                    .as_str()
                    .unwrap_or("I couldn't undo that.")
                    .to_string()
            } else {
                "Done, I've undone that.".into()
            }
        }
        Reply::Time => say_time(clock),
        Reply::Date => say_date(clock),
    }
}

/// Plays a built-in command through the normal request loop, as if a model had chosen it.
pub struct LocalModel {
    plan: Plan,
    clock: Clock,
    asked: Mutex<bool>,
}

impl LocalModel {
    pub fn new(plan: Plan, clock: Clock) -> Self {
        Self {
            plan,
            clock,
            asked: Mutex::new(false),
        }
    }
}

fn text_turn(text: String) -> ModelTurn {
    ModelTurn {
        steps: vec![json!({"type": "model_output", "content": [{"type": "text", "text": text}]})],
        calls: vec![],
        text,
    }
}

impl ChatModel for LocalModel {
    async fn respond(&self, input: &[Value]) -> Result<ModelTurn, ModelError> {
        let first = {
            let mut asked = self.asked.lock().unwrap_or_else(|p| p.into_inner());
            !std::mem::replace(&mut *asked, true)
        };
        let Some((name, args)) = &self.plan.call else {
            return Ok(text_turn(reply_text(&self.plan.reply, None, self.clock)));
        };
        if first {
            return Ok(ModelTurn {
                steps: vec![
                    json!({"type": "function_call", "id": CALL_ID, "name": name, "arguments": args}),
                ],
                calls: vec![ProposedCall {
                    id: CALL_ID.into(),
                    name: name.to_string(),
                    arguments: args.clone(),
                }],
                text: String::new(),
            });
        }
        let result = input
            .iter()
            .rev()
            .find(|s| s["type"] == "function_result" && s["call_id"] == CALL_ID);
        let text = match result {
            Some(r) if r["is_error"] == true => r["result"]["error"]
                .as_str()
                .map(|e| format!("Sorry, I couldn't do that. {e}"))
                .unwrap_or_else(|| "Sorry, that didn't work.".into()),
            Some(r) => reply_text(&self.plan.reply, Some(&r["result"]), self.clock),
            None => "Sorry, that didn't work.".into(),
        };
        Ok(text_turn(text))
    }

    fn model_id(&self) -> &'static str {
        "built-in"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::registry::validate;
    use crate::tools::Risk;

    fn apps() -> Vec<InstalledApp> {
        let a = |n: &str| InstalledApp {
            name: n.into(),
            id: format!("x!{n}"),
        };
        vec![
            a("Spotify"),
            a("Microsoft Teams"),
            a("Word"),
            a("Discord"),
            a("Terminal"),
        ]
    }

    fn call(u: &str) -> Option<(&'static str, Value)> {
        plan(u, &apps()).and_then(|p| p.call)
    }

    #[test]
    fn opens_popular_sites_apps_and_the_browser() {
        assert_eq!(
            call("Open YouTube"),
            Some(("open_website", json!({"url": "https://www.youtube.com"})))
        );
        assert_eq!(
            call("hey, could you please go to youtube for me?"),
            Some(("open_website", json!({"url": "https://www.youtube.com"})))
        );
        assert_eq!(
            call("youtube"),
            call("open youtube"),
            "a bare name works too"
        );
        assert_eq!(
            call("open spotify"),
            Some(("open_installed_app", json!({"name": "Spotify"}))),
            "the installed app wins over the website"
        );
        assert_eq!(
            call("launch teams"),
            Some(("open_installed_app", json!({"name": "Microsoft Teams"})))
        );
        assert_eq!(
            call("open my browser"),
            Some(("open_application", json!({"app": "default_browser"})))
        );
        assert_eq!(
            call("open the internet"),
            Some(("open_application", json!({"app": "default_browser"})))
        );
        assert_eq!(
            call("open file explorer"),
            Some(("open_application", json!({"app": "file_explorer"})))
        );
        assert_eq!(
            call("open gmail"),
            Some(("open_website", json!({"url": "https://mail.google.com"})))
        );
        assert_eq!(
            call("open bbc.co.uk"),
            Some(("open_website", json!({"url": "https://bbc.co.uk"})))
        );
        assert_eq!(
            plan("open spotify", &[]).and_then(|p| p.call),
            Some(("open_website", json!({"url": "https://open.spotify.com"}))),
            "no Spotify app: the web player"
        );
    }

    #[test]
    fn anything_more_specific_goes_to_the_ai() {
        for u in [
            "play Yeat on Spotify",
            "play some Kanye West",
            "open YouTube and search for Yeat",
            "open spotify then play my liked songs",
            "search for cheap flights to Rome",
            "open my last email from Anna",
            "open the photo I downloaded yesterday",
            "what does this error mean",
            "help me write an email to my son",
            "open photoshop",
            "remind me at 5 to call Anna",
            "set a timer for 30 seconds",
            "close notepad",
            "switch to spotify",
            "what's the weather like tomorrow",
            "stop",
            "continue",
            "x",
        ] {
            assert_eq!(plan(u, &apps()), None, "{u}");
        }
    }

    #[test]
    fn media_keys() {
        for (u, a) in [
            ("pause", "play_pause"),
            ("pause the music", "play_pause"),
            ("resume", "play_pause"),
            ("play", "play_pause"),
            ("next song", "next_track"),
            ("skip this song please", "next_track"),
            ("go back a song", "previous_track"),
            ("turn it up", "volume_up"),
            ("Louder!", "volume_up"),
            ("turn the volume down", "volume_down"),
            ("mute", "mute"),
        ] {
            assert_eq!(
                call(u),
                Some(("media_control", json!({"action": a}))),
                "{u}"
            );
        }
    }

    #[test]
    fn timers_and_reminders() {
        assert_eq!(
            call("set a timer for 10 minutes"),
            Some((
                "set_reminder",
                json!({"text": "Your timer for 10 minutes is done", "in_minutes": 10})
            ))
        );
        assert_eq!(call("set a ten minute timer").unwrap().1["in_minutes"], 10);
        assert_eq!(call("timer for an hour").unwrap().1["in_minutes"], 60);
        assert_eq!(
            call("set a timer for half an hour").unwrap().1["in_minutes"],
            30
        );
        assert_eq!(
            call("set a timer for twenty five minutes").unwrap().1["in_minutes"],
            25
        );
        assert_eq!(
            call("remind me in 20 minutes to take my pills"),
            Some((
                "set_reminder",
                json!({"text": "take my pills", "in_minutes": 20})
            ))
        );
        assert_eq!(
            call("Remind me to call Anna in two hours."),
            Some((
                "set_reminder",
                json!({"text": "call anna", "in_minutes": 120})
            ))
        );
        assert_eq!(
            call("what reminders do I have?"),
            Some(("list_reminders", json!({})))
        );
        assert_eq!(call("set a timer for 2000 minutes"), None);
    }

    #[test]
    fn status_windows_time_and_more() {
        assert_eq!(
            call("how much battery do I have?"),
            Some(("computer_status", json!({})))
        );
        assert_eq!(call("am I online"), Some(("computer_status", json!({}))));
        assert_eq!(
            call("minimize this"),
            Some(("manage_window", json!({"action": "minimize"})))
        );
        assert_eq!(
            call("make it full screen"),
            Some(("manage_window", json!({"action": "maximize"})))
        );
        assert_eq!(call("undo that"), Some(("undo_last_action", json!({}))));
        assert_eq!(
            call("turn off the magnifier"),
            Some(("screen_magnifier", json!({"action": "turn_off"})))
        );
        assert_eq!(
            plan("what time is it?", &[]).map(|p| p.reply),
            Some(Reply::Time)
        );
        assert_eq!(
            plan("What's the date today", &[]).map(|p| p.reply),
            Some(Reply::Date)
        );
    }

    #[test]
    fn every_built_in_command_is_valid_and_never_needs_confirmation() {
        let samples = [
            "open youtube",
            "open spotify",
            "open teams",
            "open terminal",
            "open browser",
            "open notepad",
            "open bbc.co.uk",
            "pause",
            "next song",
            "turn it up",
            "mute",
            "set a timer for 5 minutes",
            "remind me in 1 hour to stretch",
            "what reminders do i have",
            "battery",
            "am i online",
            "how much space is left",
            "minimize",
            "full screen",
            "restore",
            "turn on the magnifier",
            "turn off the magnifier",
            "undo",
        ];
        for u in samples {
            let (name, args) = call(u).unwrap_or_else(|| panic!("{u} should be built in"));
            let v = validate(name, &args, false).unwrap_or_else(|e| panic!("{u}: {e:?}"));
            assert_ne!(v.risk(), Risk::High, "{u}");
        }
        for (names, _, url) in SITES {
            assert!(
                crate::tools::web_url::validate_url(url).is_ok(),
                "{names:?}"
            );
        }
    }

    #[test]
    fn answers_are_worded_from_what_happened() {
        let clock = Clock {
            year: 2026,
            month: 10,
            day_of_week: 0,
            day: 18,
            hour: 21,
            minute: 5,
        };
        assert_eq!(reply_text(&Reply::Time, None, clock), "It's 9:05 PM.");
        assert_eq!(
            reply_text(&Reply::Date, None, clock),
            "Today is Sunday, 18 October 2026."
        );
        assert_eq!(
            reply_text(
                &Reply::Status(StatusTopic::Battery),
                Some(&json!({"has_battery": true, "battery_percent": 64, "plugged_in": false})),
                clock
            ),
            "Your battery is at 64 percent and not plugged in."
        );
        assert_eq!(
            reply_text(
                &Reply::Timer { minutes: 90 },
                Some(&json!({"due_at_local_time": "22:35"})),
                clock
            ),
            "Timer set for 1 hour and 30 minutes. It'll go off at 22:35. Keep this app open for it to ring."
        );
        assert_eq!(
            reply_text(
                &Reply::ListReminders,
                Some(&json!({"reminders_soonest_first": []})),
                clock
            ),
            "You don't have any reminders or timers set."
        );
    }
}
