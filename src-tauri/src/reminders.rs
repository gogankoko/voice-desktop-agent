//! Timers and reminders ("remind me in 20 minutes to take my pills").
//!
//! Kept in memory while the app is open (they are lost if it closes; the assistant says so).
//! A background loop checks once a second; a due reminder is sent to the window, which
//! chimes, shows it and reads it aloud. Only the person's own words are stored; nothing is
//! sent anywhere when a reminder fires, and the log never contains the text.

use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime};

use serde::Deserialize;

pub const MAX_ACTIVE: usize = 20;
pub const MAX_MINUTES: u32 = 24 * 60;
pub const MAX_TEXT_CHARS: usize = 120;

/// ReminderTime a reminder is due, as the model asked for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
pub enum ReminderTime {
    InMinutes(u32),
    /// Local clock time, 24-hour; the next time it comes round (today or tomorrow).
    At {
        hour: u8,
        minute: u8,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reminder {
    pub id: u32,
    pub text: String,
    pub due: SystemTime,
    /// Local clock time it's due, "HH:MM".
    pub due_clock: String,
}

#[derive(Default)]
pub struct Reminders {
    inner: Mutex<(u32, Vec<Reminder>)>,
}

impl Reminders {
    pub fn add(
        &self,
        text: String,
        secs_from_now: u64,
        now: SystemTime,
        due_clock: String,
    ) -> Result<Reminder, String> {
        let mut g = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        if g.1.len() >= MAX_ACTIVE {
            return Err(format!(
                "There are already {MAX_ACTIVE} reminders set. Cancel one first."
            ));
        }
        g.0 += 1;
        let r = Reminder {
            id: g.0,
            text,
            due: now + Duration::from_secs(secs_from_now),
            due_clock,
        };
        g.1.push(r.clone());
        Ok(r)
    }

    /// Active reminders, soonest first.
    pub fn list(&self) -> Vec<Reminder> {
        let g = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        let mut v = g.1.clone();
        v.sort_by_key(|r| r.due);
        v
    }

    pub fn cancel(&self, id: u32) -> Option<Reminder> {
        let mut g = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        let i = g.1.iter().position(|r| r.id == id)?;
        Some(g.1.remove(i))
    }

    /// Removes and returns every reminder due at `now`.
    pub fn take_due(&self, now: SystemTime) -> Vec<Reminder> {
        let mut g = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        let (due, later): (Vec<_>, Vec<_>) = g.1.drain(..).partition(|r| r.due <= now);
        g.1 = later;
        due
    }
}

/// Seconds from now (local h:m:s) until the next time the clock shows `at_h:at_m`.
pub fn secs_until(now_h: u16, now_m: u16, now_s: u16, at_h: u8, at_m: u8) -> u64 {
    let now = u64::from(now_h) * 3600 + u64::from(now_m) * 60 + u64::from(now_s);
    let at = u64::from(at_h) * 3600 + u64::from(at_m) * 60;
    if at > now {
        at - now
    } else {
        at + 86_400 - now
    }
}

/// The local clock time ("HH:MM") `secs` after now.
pub fn clock_after(now_h: u16, now_m: u16, now_s: u16, secs: u64) -> String {
    let t = (u64::from(now_h) * 3600 + u64::from(now_m) * 60 + u64::from(now_s) + secs) % 86_400;
    format!("{:02}:{:02}", t / 3600, (t % 3600) / 60)
}

/// "17:30" or "7:05" -> (17, 30) / (7, 5).
pub fn parse_clock(s: &str) -> Option<(u8, u8)> {
    let (h, m) = s.trim().split_once(':')?;
    if h.is_empty() || h.len() > 2 || m.len() != 2 {
        return None;
    }
    let h: u8 = h.parse().ok()?;
    let m: u8 = m.parse().ok()?;
    (h < 24 && m < 60).then_some((h, m))
}

/// Computes when a reminder is due from the local clock; returns (seconds from now, "HH:MM").
pub fn schedule(when: ReminderTime, now_h: u16, now_m: u16, now_s: u16) -> (u64, String) {
    let secs = match when {
        ReminderTime::InMinutes(n) => u64::from(n) * 60,
        ReminderTime::At { hour, minute } => secs_until(now_h, now_m, now_s, hour, minute),
    };
    (secs, clock_after(now_h, now_m, now_s, secs))
}

/// The app-wide reminder list.
pub fn store() -> &'static Reminders {
    static STORE: OnceLock<Reminders> = OnceLock::new();
    STORE.get_or_init(Reminders::default)
}

/// Starts the once-a-second check that delivers due reminders to the window.
pub fn start(app: tauri::AppHandle) {
    use tauri::{Emitter, Manager};
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(1)).await;
            for r in store().take_due(SystemTime::now()) {
                crate::diag::log("reminders", format!("reminder {} is due", r.id));
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.unminimize();
                    let _ = w.show();
                    let _ = w.request_user_attention(Some(tauri::UserAttentionType::Critical));
                }
                let _ = app.emit("reminder-due", serde_json::json!({ "text": r.text }));
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_times_roll_over_to_tomorrow() {
        assert_eq!(secs_until(9, 0, 0, 9, 30), 30 * 60);
        assert_eq!(secs_until(9, 0, 30, 9, 1), 30);
        assert_eq!(secs_until(18, 0, 0, 9, 0), 15 * 3600);
        assert_eq!(
            secs_until(9, 0, 0, 9, 0),
            86_400,
            "now means tomorrow, not instantly"
        );
        assert_eq!(clock_after(23, 50, 0, 20 * 60), "00:10");
        assert_eq!(
            schedule(ReminderTime::InMinutes(20), 14, 5, 0),
            (1200, "14:25".into())
        );
        assert_eq!(
            schedule(
                ReminderTime::At {
                    hour: 17,
                    minute: 30
                },
                17,
                0,
                0
            )
            .1,
            "17:30"
        );
    }

    #[test]
    fn parses_only_real_clock_times() {
        assert_eq!(parse_clock("17:30"), Some((17, 30)));
        assert_eq!(parse_clock(" 7:05 "), Some((7, 5)));
        for bad in [
            "24:00", "12:60", "5pm", "12:5", "", ":30", "123:00", "-1:00",
        ] {
            assert_eq!(parse_clock(bad), None, "{bad}");
        }
    }

    #[test]
    fn reminders_fire_once_in_order_and_can_be_cancelled() {
        let r = Reminders::default();
        let t0 = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
        let a = r.add("pills".into(), 600, t0, "10:10".into()).unwrap();
        let b = r.add("tea".into(), 60, t0, "10:01".into()).unwrap();
        let c = r.add("call Anna".into(), 120, t0, "10:02".into()).unwrap();
        assert_eq!(
            r.list().iter().map(|x| x.id).collect::<Vec<_>>(),
            vec![b.id, c.id, a.id]
        );
        assert_eq!(r.cancel(c.id).map(|x| x.text), Some("call Anna".into()));
        assert_eq!(r.cancel(c.id), None);
        assert!(r.take_due(t0 + Duration::from_secs(59)).is_empty());
        let due = r.take_due(t0 + Duration::from_secs(60));
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].text, "tea");
        assert!(
            r.take_due(t0 + Duration::from_secs(61)).is_empty(),
            "fires once"
        );
        assert_eq!(r.list().len(), 1);
    }

    #[test]
    fn at_most_twenty_active() {
        let r = Reminders::default();
        let t0 = SystemTime::UNIX_EPOCH;
        for i in 0..MAX_ACTIVE {
            r.add(format!("r{i}"), 60, t0, "00:01".into()).unwrap();
        }
        assert!(
            r.add("one too many".into(), 60, t0, "00:01".into())
                .is_err()
        );
    }
}
