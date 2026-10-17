use serde::Serialize;

/// Availability of an individual subsystem.
#[allow(dead_code)] // `Mock` is reserved for the practice providers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Availability {
    /// Not built yet in this version.
    NotImplemented,
    /// Works once the user adds their provider details in Settings.
    NeedsSetup,
    /// Simulated; never performs real actions.
    Mock,
    /// Some real actions work; the UI says which.
    Limited,
    /// Real integration is available.
    Available,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppStatus {
    pub app_version: &'static str,
    pub os: &'static str,
    pub speech_recognition: Availability,
    pub speech_synthesis: Availability,
    pub reasoning_model: Availability,
    pub computer_control: Availability,
}

/// `stt_configured`: an Azure Speech key is stored in Credential Manager.
pub fn current_status(stt_configured: bool) -> AppStatus {
    AppStatus {
        app_version: env!("CARGO_PKG_VERSION"),
        os: std::env::consts::OS,
        speech_recognition: if stt_configured {
            Availability::Available
        } else {
            Availability::NeedsSetup
        },
        // Windows voices through WebView2; the UI downgrades this if no voice is installed.
        speech_synthesis: Availability::Available,
        reasoning_model: Availability::NotImplemented,
        computer_control: Availability::Limited,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_does_not_overclaim_capabilities() {
        let s = current_status(false);
        assert_eq!(s.computer_control, Availability::Limited);
        assert_eq!(s.reasoning_model, Availability::NotImplemented);
        assert_eq!(s.speech_recognition, Availability::NeedsSetup);
        assert_eq!(
            current_status(true).speech_recognition,
            Availability::Available
        );
    }

    #[test]
    fn status_serialises_with_camel_case_and_snake_case_values() {
        let json = serde_json::to_value(current_status(false)).unwrap();
        assert_eq!(json["appVersion"], env!("CARGO_PKG_VERSION"));
        assert_eq!(json["computerControl"], "limited");
    }
}
