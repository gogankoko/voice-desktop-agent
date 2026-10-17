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

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Preferences {
    pub text_size: TextSize,
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
        let text_size: Option<String> = conn
            .query_row(
                "SELECT value FROM preferences WHERE key = 'text_size'",
                [],
                |r| r.get(0),
            )
            .optional()?;
        Ok(Preferences {
            text_size: text_size
                .as_deref()
                .and_then(TextSize::parse)
                .unwrap_or_default(),
        })
    }

    pub fn save(&self, prefs: &Preferences) -> rusqlite::Result<()> {
        let conn = self.conn.lock().expect("prefs mutex poisoned");
        conn.execute(
            "INSERT INTO preferences (key, value) VALUES ('text_size', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![prefs.text_size.as_str()],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_when_nothing_saved() {
        let store = PrefsStore::open_in_memory().unwrap();
        assert_eq!(store.load().unwrap(), Preferences::default());
    }

    #[test]
    fn round_trips_and_overwrites() {
        let store = PrefsStore::open_in_memory().unwrap();
        store
            .save(&Preferences {
                text_size: TextSize::Large,
            })
            .unwrap();
        store
            .save(&Preferences {
                text_size: TextSize::ExtraLarge,
            })
            .unwrap();
        assert_eq!(store.load().unwrap().text_size, TextSize::ExtraLarge);
    }

    #[test]
    fn corrupt_stored_value_falls_back_to_default() {
        let store = PrefsStore::open_in_memory().unwrap();
        store
            .conn
            .lock()
            .unwrap()
            .execute(
                "INSERT INTO preferences VALUES ('text_size', 'gigantic')",
                [],
            )
            .unwrap();
        assert_eq!(store.load().unwrap().text_size, TextSize::Normal);
    }

    #[test]
    fn persists_across_reopen() {
        let dir = std::env::temp_dir().join(format!("prefs-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test.db");
        PrefsStore::open(&path)
            .unwrap()
            .save(&Preferences {
                text_size: TextSize::Large,
            })
            .unwrap();
        assert_eq!(
            PrefsStore::open(&path).unwrap().load().unwrap().text_size,
            TextSize::Large
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn rejects_invalid_input_from_frontend() {
        assert!(serde_json::from_str::<Preferences>(r#"{"textSize":"huge"}"#).is_err());
        assert!(serde_json::from_str::<Preferences>(r#"{"textSize":1.4}"#).is_err());
        assert!(
            serde_json::from_str::<Preferences>(r#"{"textSize":"large","extra":true}"#).is_err()
        );
    }
}
