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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Preferences {
    pub text_size: TextSize,
    /// Read the assistant's replies aloud.
    pub speak_replies: bool,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            text_size: TextSize::Normal,
            speak_replies: true,
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
        })
    }

    pub fn save(&self, prefs: &Preferences) -> rusqlite::Result<()> {
        let mut conn = self.conn.lock().expect("prefs mutex poisoned");
        let tx = conn.transaction()?;
        for (key, value) in [
            ("text_size", prefs.text_size.as_str()),
            ("speak_replies", if prefs.speak_replies { "1" } else { "0" }),
        ] {
            tx.execute(
                "INSERT INTO preferences (key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![key, value],
            )?;
        }
        tx.commit()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prefs(text_size: TextSize, speak_replies: bool) -> Preferences {
        Preferences {
            text_size,
            speak_replies,
        }
    }

    #[test]
    fn defaults_when_nothing_saved() {
        let store = PrefsStore::open_in_memory().unwrap();
        assert_eq!(store.load().unwrap(), Preferences::default());
        assert!(
            Preferences::default().speak_replies,
            "replies are spoken by default"
        );
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
            r#"{"textSize":"large"}"#,
            r#"{"textSize":"large","speakReplies":"yes"}"#,
            r#"{"textSize":"huge","speakReplies":true}"#,
            r#"{"textSize":1.4,"speakReplies":true}"#,
            r#"{"textSize":"large","speakReplies":true,"extra":true}"#,
        ] {
            assert!(serde_json::from_str::<Preferences>(bad).is_err(), "{bad}");
        }
    }
}
