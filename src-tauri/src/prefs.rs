//! User preferences persisted in a local SQLite database.
//! Only small, non-sensitive settings live here (see SECURITY.md "Data handling").

use std::path::Path;
use std::sync::Mutex;

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextSize {
    #[default]
    Normal,
    Large,
    ExtraLarge,
}

impl TextSize {
    fn as_str(self) -> &'static str {
        match self {
            TextSize::Normal => "normal",
            TextSize::Large => "large",
            TextSize::ExtraLarge => "extra_large",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        match s {
            "normal" => Some(TextSize::Normal),
            "large" => Some(TextSize::Large),
            "extra_large" => Some(TextSize::ExtraLarge),
            _ => None,
        }
    }
}

/// The voice that reads replies aloud: a natural Azure voice, or the Windows voice.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Voice {
    #[default]
    Ava,
    Andrew,
    Emma,
    Brian,
    Sonia,
    Ryan,
    Windows,
}

impl Voice {
    const ALL: [Voice; 7] = [
        Voice::Ava,
        Voice::Andrew,
        Voice::Emma,
        Voice::Brian,
        Voice::Sonia,
        Voice::Ryan,
        Voice::Windows,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Voice::Ava => "ava",
            Voice::Andrew => "andrew",
            Voice::Emma => "emma",
            Voice::Brian => "brian",
            Voice::Sonia => "sonia",
            Voice::Ryan => "ryan",
            Voice::Windows => "windows",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|v| v.as_str() == s)
    }

    /// Azure's name for the voice (None: the Windows voice).
    pub fn azure_name(self) -> Option<&'static str> {
        Some(match self {
            Voice::Ava => "en-US-AvaMultilingualNeural",
            Voice::Andrew => "en-US-AndrewMultilingualNeural",
            Voice::Emma => "en-US-EmmaMultilingualNeural",
            Voice::Brian => "en-US-BrianMultilingualNeural",
            Voice::Sonia => "en-GB-SoniaNeural",
            Voice::Ryan => "en-GB-RyanNeural",
            Voice::Windows => return None,
        })
    }
}

pub const DEFAULT_NAME: &str = "Nova";

/// A name the assistant can be called: one or two words of letters (also - and '), at most
/// 20 characters. Returns the tidied name.
pub fn normalise_name(name: &str) -> Result<String, String> {
    let n = name.split_whitespace().collect::<Vec<_>>().join(" ");
    let ok = (2..=20).contains(&n.chars().count())
        && n.split(' ').count() <= 2
        && n.chars()
            .all(|c| c.is_alphabetic() || c == ' ' || c == '-' || c == '\'')
        && n.chars().next().is_some_and(char::is_alphabetic);
    if ok {
        Ok(n)
    } else {
        Err(
            "The name should be one or two words of letters, up to 20 characters, like \"Nova\"."
                .into(),
        )
    }
}

static CURRENT_NAME: std::sync::RwLock<Option<String>> = std::sync::RwLock::new(None);

/// The assistant's current name (for the AI's instructions).
pub fn assistant_name() -> String {
    CURRENT_NAME
        .read()
        .unwrap_or_else(|p| p.into_inner())
        .clone()
        .unwrap_or_else(|| DEFAULT_NAME.to_string())
}

fn remember_name(name: &str) {
    *CURRENT_NAME.write().unwrap_or_else(|p| p.into_inner()) = Some(name.to_string());
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Preferences {
    pub text_size: TextSize,
    /// Read the assistant's replies aloud.
    pub speak_replies: bool,
    /// Offer pretend high-risk actions for practising confirmations. Nothing real happens.
    pub practice_mode: bool,
    /// Let the assistant search file names in Downloads, Pictures, Documents and Desktop.
    pub file_search: bool,
    /// The voice for replies (natural Azure voices need the Azure Speech key).
    pub voice: Voice,
    /// What the assistant is called (and answers to).
    pub assistant_name: String,
    /// Listen for the name on this computer, to start listening without a button.
    pub wake_word: bool,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            text_size: TextSize::Normal,
            speak_replies: true,
            practice_mode: false,
            file_search: false,
            voice: Voice::Ava,
            assistant_name: DEFAULT_NAME.to_string(),
            wake_word: false,
        }
    }
}

pub struct PrefsStore {
    conn: Mutex<Connection>,
}

impl PrefsStore {
    pub fn open(path: &Path) -> rusqlite::Result<Self> {
        Self::init(Connection::open(path)?)
    }

    #[cfg(test)]
    pub fn open_in_memory() -> rusqlite::Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> rusqlite::Result<Self> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS preferences (
                key   TEXT PRIMARY KEY NOT NULL,
                value TEXT NOT NULL
            );",
        )?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Missing or unrecognised stored values fall back to defaults rather than failing.
    pub fn load(&self) -> rusqlite::Result<Preferences> {
        let conn = self.conn.lock().expect("prefs mutex poisoned");
        let get = |key: &str| -> rusqlite::Result<Option<String>> {
            conn.query_row("SELECT value FROM preferences WHERE key = ?1", [key], |r| {
                r.get(0)
            })
            .optional()
        };
        let defaults = Preferences::default();
        Ok(Preferences {
            text_size: get("text_size")?
                .as_deref()
                .and_then(TextSize::parse)
                .unwrap_or(defaults.text_size),
            speak_replies: match get("speak_replies")?.as_deref() {
                Some("1") => true,
                Some("0") => false,
                _ => defaults.speak_replies,
            },
            practice_mode: match get("practice_mode")?.as_deref() {
                Some("1") => true,
                Some("0") => false,
                _ => defaults.practice_mode,
            },
            file_search: match get("file_search")?.as_deref() {
                Some("1") => true,
                Some("0") => false,
                _ => defaults.file_search,
            },
            voice: get("voice")?
                .as_deref()
                .and_then(Voice::parse)
                .unwrap_or(defaults.voice),
            assistant_name: get("assistant_name")?
                .as_deref()
                .and_then(|n| normalise_name(n).ok())
                .unwrap_or(defaults.assistant_name),
            wake_word: match get("wake_word")?.as_deref() {
                Some("1") => true,
                Some("0") => false,
                _ => defaults.wake_word,
            },
        })
        .inspect(|p| remember_name(&p.assistant_name))
    }

    pub fn save(&self, prefs: &Preferences) -> rusqlite::Result<()> {
        let mut conn = self.conn.lock().expect("prefs mutex poisoned");
        let tx = conn.transaction()?;
        for (key, value) in [
            ("text_size", prefs.text_size.as_str()),
            ("speak_replies", if prefs.speak_replies { "1" } else { "0" }),
            ("practice_mode", if prefs.practice_mode { "1" } else { "0" }),
            ("file_search", if prefs.file_search { "1" } else { "0" }),
            ("voice", prefs.voice.as_str()),
            ("assistant_name", prefs.assistant_name.as_str()),
            ("wake_word", if prefs.wake_word { "1" } else { "0" }),
        ] {
            tx.execute(
                "INSERT INTO preferences (key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![key, value],
            )?;
        }
        tx.commit()?;
        remember_name(&prefs.assistant_name);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prefs(text_size: TextSize, speak_replies: bool) -> Preferences {
        Preferences {
            text_size,
            speak_replies,
            practice_mode: !speak_replies,
            file_search: speak_replies,
            voice: if speak_replies {
                Voice::Brian
            } else {
                Voice::Windows
            },
            assistant_name: if speak_replies { "Jarvis" } else { "Mister T" }.into(),
            wake_word: speak_replies,
        }
    }

    #[test]
    fn names_are_tidied_and_restricted() {
        assert_eq!(normalise_name("  Nova ").unwrap(), "Nova");
        assert_eq!(normalise_name("Mister   T").unwrap(), "Mister T");
        assert_eq!(normalise_name("Zoë").unwrap(), "Zoë");
        assert_eq!(normalise_name("O'Neil").unwrap(), "O'Neil");
        for bad in [
            "",
            "N",
            "Hey there my friend",
            "R2D2",
            "<script>",
            "Nova!",
            "-Nova",
            &"a".repeat(21),
        ] {
            assert!(normalise_name(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn every_voice_round_trips_and_only_windows_has_no_azure_name() {
        for v in Voice::ALL {
            assert_eq!(Voice::parse(v.as_str()), Some(v));
            assert_eq!(v.azure_name().is_none(), v == Voice::Windows);
        }
        assert_eq!(Voice::parse("en-US-Evil"), None);
    }

    #[test]
    fn defaults_when_nothing_saved() {
        let store = PrefsStore::open_in_memory().unwrap();
        assert_eq!(store.load().unwrap(), Preferences::default());
        assert!(
            Preferences::default().speak_replies,
            "replies are spoken by default"
        );
        assert!(
            !Preferences::default().practice_mode,
            "practice mode is off by default"
        );
        assert!(!Preferences::default().file_search, "file search is opt-in");
    }

    #[test]
    fn round_trips_and_overwrites() {
        let store = PrefsStore::open_in_memory().unwrap();
        store.save(&prefs(TextSize::Large, true)).unwrap();
        store.save(&prefs(TextSize::ExtraLarge, false)).unwrap();
        assert_eq!(store.load().unwrap(), prefs(TextSize::ExtraLarge, false));
    }

    #[test]
    fn corrupt_stored_values_fall_back_to_defaults() {
        let store = PrefsStore::open_in_memory().unwrap();
        store
            .conn
            .lock()
            .unwrap()
            .execute_batch(
                "INSERT INTO preferences VALUES ('text_size', 'gigantic');
                 INSERT INTO preferences VALUES ('speak_replies', 'maybe');",
            )
            .unwrap();
        assert_eq!(store.load().unwrap(), Preferences::default());
    }

    #[test]
    fn persists_across_reopen() {
        let dir = std::env::temp_dir().join(format!("prefs-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test.db");
        PrefsStore::open(&path)
            .unwrap()
            .save(&prefs(TextSize::Large, false))
            .unwrap();
        assert_eq!(
            PrefsStore::open(&path).unwrap().load().unwrap(),
            prefs(TextSize::Large, false)
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn rejects_invalid_input_from_frontend() {
        for bad in [
            r#"{"textSize":"large","speakReplies":true}"#,
            r#"{"textSize":"large","speakReplies":"yes","practiceMode":false,"fileSearch":false}"#,
            r#"{"textSize":"huge","speakReplies":true,"practiceMode":false,"fileSearch":false}"#,
            r#"{"textSize":1.4,"speakReplies":true,"practiceMode":false,"fileSearch":false}"#,
            r#"{"textSize":"large","speakReplies":true,"practiceMode":false,"fileSearch":false,"extra":true}"#,
        ] {
            assert!(serde_json::from_str::<Preferences>(bad).is_err(), "{bad}");
        }
    }
}
