//! Wake word: "Hey Nova" (or whatever the assistant is called) starts listening, like the
//! microphone button.
//!
//! Detection is done on this computer by Windows' own offline speech recognition, which only
//! listens for the few fixed phrases built from the assistant's name. Nothing is sent
//! anywhere and nothing is recorded until a phrase is heard; then the app listens through
//! Azure exactly as when the button is pressed. Off unless the person turns it on.

use std::sync::mpsc::{self, Sender};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WakeStatus {
    /// "off", "listening" or "unavailable".
    pub state: &'static str,
    /// What it listens for, or why it can't.
    pub detail: Option<String>,
}

enum Msg {
    Configure(Option<Vec<String>>),
    /// Session number `n` ended (e.g. the microphone changed). Only the current session's
    /// end restarts it; ends caused by stopping an old session on purpose are ignored.
    Ended(u64),
}

static TX: OnceLock<Mutex<Sender<Msg>>> = OnceLock::new();
static STATUS: Mutex<Option<WakeStatus>> = Mutex::new(None);

/// The phrases that wake the assistant called `name`.
pub fn phrases(name: &str) -> Vec<String> {
    let n = name.trim().to_lowercase();
    vec![
        format!("hey {n}"),
        format!("ok {n}"),
        format!("okay {n}"),
        format!("hi {n}"),
        n,
    ]
}

pub fn status() -> WakeStatus {
    STATUS
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .clone()
        .unwrap_or(WakeStatus {
            state: "off",
            detail: None,
        })
}

fn set_status(app: &AppHandle, s: WakeStatus) {
    *STATUS.lock().unwrap_or_else(|p| p.into_inner()) = Some(s.clone());
    let _ = app.emit("wake-status", s);
}

/// Turns listening for the name on or off (and changes the name).
pub fn configure(enabled: bool, name: &str) {
    if let Some(tx) = TX.get() {
        let _ = tx
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .send(Msg::Configure(enabled.then(|| phrases(name))));
    }
}

/// Starts the background listener thread (idle until `configure` turns it on).
pub fn start(app: AppHandle) {
    let (tx, rx) = mpsc::channel::<Msg>();
    let _ = TX.set(Mutex::new(tx.clone()));
    std::thread::spawn(move || {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(
                None,
                windows::Win32::System::Com::COINIT_MULTITHREADED,
            );
        }
        let mut current: Option<imp::Running> = None;
        let mut wanted: Option<Vec<String>> = None;
        let mut session = 0u64;
        while let Ok(msg) = rx.recv() {
            match msg {
                Msg::Configure(p) => wanted = p,
                Msg::Ended(n) if n != session => continue,
                Msg::Ended(_) => {
                    // Avoid a tight loop if the session keeps ending at once.
                    std::thread::sleep(Duration::from_secs(2));
                }
            }
            session += 1;
            if let Some(r) = current.take() {
                r.stop();
            }
            let Some(p) = wanted.clone() else {
                set_status(
                    &app,
                    WakeStatus {
                        state: "off",
                        detail: None,
                    },
                );
                continue;
            };
            let app2 = app.clone();
            match imp::listen(&p, tx.clone(), session, move || woken(&app2)) {
                Ok(r) => {
                    current = Some(r);
                    set_status(
                        &app,
                        WakeStatus {
                            state: "listening",
                            detail: Some(p[0].clone()),
                        },
                    );
                }
                Err(why) => {
                    crate::diag::log("wake", format!("can't listen for the name: {why}"));
                    set_status(
                        &app,
                        WakeStatus {
                            state: "unavailable",
                            detail: Some(why),
                        },
                    );
                }
            }
        }
    });
}

/// A phrase was heard: bring the window forward and tell it to listen (at most every 2 s).
fn woken(app: &AppHandle) {
    static LAST: Mutex<Option<Instant>> = Mutex::new(None);
    {
        let mut last = LAST.lock().unwrap_or_else(|p| p.into_inner());
        if last.is_some_and(|t| t.elapsed() < Duration::from_secs(2)) {
            return;
        }
        *last = Some(Instant::now());
    }
    crate::diag::log("wake", "heard the name");
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
    let _ = app.emit("wake-word", ());
}

mod imp {
    use std::sync::mpsc::Sender;

    use windows::Foundation::TypedEventHandler;
    use windows::Media::SpeechRecognition::{
        SpeechContinuousRecognitionCompletedEventArgs,
        SpeechContinuousRecognitionResultGeneratedEventArgs, SpeechContinuousRecognitionSession,
        SpeechRecognitionConfidence, SpeechRecognitionListConstraint,
        SpeechRecognitionResultStatus, SpeechRecognizer,
    };
    use windows::core::HSTRING;
    use windows_collections::IIterable;

    use super::Msg;

    pub struct Running {
        _recognizer: SpeechRecognizer,
        session: SpeechContinuousRecognitionSession,
    }

    impl Running {
        pub fn stop(self) {
            if let Ok(op) = self.session.StopAsync() {
                let _ = op.join();
            }
        }
    }

    fn plain(e: windows::core::Error) -> String {
        let code = e.code().0 as u32;
        match code {
            // SPERR_NOT_FOUND / language not installed.
            0x8004503A | 0x800455BC => {
                "Windows' English speech recognition isn't installed (Settings > Time & language > Speech).".into()
            }
            // Microphone access denied.
            0x80045509 | 0x80070005 => {
                "Windows didn't allow access to the microphone (Settings > Privacy & security > Microphone).".into()
            }
            _ => format!("Windows speech recognition isn't available (code {code:#x})."),
        }
    }

    pub fn listen(
        phrases: &[String],
        tx: Sender<Msg>,
        session_number: u64,
        on_heard: impl Fn() + Send + 'static,
    ) -> Result<Running, String> {
        let recognizer = SpeechRecognizer::new().map_err(plain)?;
        let list: Vec<HSTRING> = phrases.iter().map(HSTRING::from).collect();
        let list: IIterable<HSTRING> = list.into();
        let constraint =
            SpeechRecognitionListConstraint::CreateWithTag(&list, &HSTRING::from("wake"))
                .map_err(plain)?;
        recognizer
            .Constraints()
            .and_then(|c| c.Append(&constraint))
            .map_err(plain)?;
        let compiled = recognizer
            .CompileConstraintsAsync()
            .and_then(|op| op.join())
            .map_err(plain)?;
        if compiled.Status().map_err(plain)? != SpeechRecognitionResultStatus::Success {
            return Err(
                "Windows couldn't set up listening for the name (is English speech recognition installed?)."
                    .into(),
            );
        }
        let session = recognizer.ContinuousRecognitionSession().map_err(plain)?;
        // By default a session stops after 20 s of silence; keep it going.
        let _ = session.SetAutoStopSilenceTimeout(windows::Foundation::TimeSpan {
            Duration: 10_000_000 * 60 * 60 * 24 * 365,
        });
        session
            .ResultGenerated(&TypedEventHandler::<
                SpeechContinuousRecognitionSession,
                SpeechContinuousRecognitionResultGeneratedEventArgs,
            >::new(move |_, args| {
                if let Some(args) = args.as_ref() {
                    let confidence = args.Result()?.Confidence()?;
                    #[cfg(test)]
                    eprintln!(
                        "  heard {:?} (confidence {:?})",
                        args.Result()?.Text()?.to_string(),
                        confidence.0
                    );
                    if confidence == SpeechRecognitionConfidence::High
                        || confidence == SpeechRecognitionConfidence::Medium
                    {
                        on_heard();
                    }
                }
                Ok(())
            }))
            .map_err(plain)?;
        session
            .Completed(&TypedEventHandler::<
                SpeechContinuousRecognitionSession,
                SpeechContinuousRecognitionCompletedEventArgs,
            >::new(move |_, _| {
                let _ = tx.send(Msg::Ended(session_number));
                Ok(())
            }))
            .map_err(plain)?;
        session
            .StartAsync()
            .and_then(|op| op.join())
            .map_err(plain)?;
        Ok(Running {
            _recognizer: recognizer,
            session,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// AUDIBLE: listens for "Hey Nova" through the microphone while a Windows voice says it
    /// through the speakers (needs speakers on and the microphone able to hear them).
    /// cargo test --manifest-path src-tauri/Cargo.toml live_wake -- --ignored --nocapture --test-threads=1
    #[test]
    #[ignore = "uses the microphone and speakers"]
    fn live_wake_word() {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(
                None,
                windows::Win32::System::Com::COINIT_MULTITHREADED,
            );
        }
        let (heard_tx, heard_rx) = mpsc::channel();
        let (tx, _rx) = mpsc::channel();
        let running = imp::listen(&phrases("Nova"), tx, 1, move || {
            let _ = heard_tx.send(());
        })
        .expect("Windows speech recognition");
        println!("listening for 'hey nova'");
        std::thread::sleep(Duration::from_secs(1));
        for line in ["The weather is nice today.", "Hey Nova", "Hey Nova"] {
            let _ = std::process::Command::new("powershell")
                .args([
                    "-NoProfile",
                    "-Command",
                    &format!("$v = New-Object -ComObject SAPI.SpVoice; $v.Volume = 100; [void]$v.Speak('{line}')"),
                ])
                .status();
            if heard_rx.recv_timeout(Duration::from_secs(2)).is_ok() {
                println!("heard after: {line}");
                running.stop();
                assert_ne!(
                    line, "The weather is nice today.",
                    "woke on the wrong words"
                );
                return;
            }
            println!("not heard after: {line}");
        }
        running.stop();
        panic!("never heard 'hey nova' (speakers may be off or the microphone can't hear them)");
    }

    #[test]
    fn phrases_come_from_the_name() {
        assert_eq!(
            phrases(" Nova "),
            vec!["hey nova", "ok nova", "okay nova", "hi nova", "nova"]
        );
        assert_eq!(phrases("Mister T")[0], "hey mister t");
    }
}
