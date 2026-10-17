use serde::Serialize;

/// Availability of an individual subsystem.
#[allow(dead_code)] // `Mock` and `Available` are used once providers land (Phase 3+).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Availability {
    /// Not built yet in this version.
    NotImplemented,
    /// Simulated; never performs real actions.
    Mock,
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

pub fn current_status() -> AppStatus {
    AppStatus {
        app_version: env!("CARGO_PKG_VERSION"),
        os: std::env::consts::OS,
        speech_recognition: Availability::NotImplemented,
        speech_synthesis: Availability::NotImplemented,
        reasoning_model: Availability::NotImplemented,
        computer_control: Availability::NotImplemented,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_does_not_overclaim_capabilities() {
        let s = current_status();
        assert_eq!(s.computer_control, Availability::NotImplemented);
        assert_eq!(s.reasoning_model, Availability::NotImplemented);
    }

    #[test]
    fn status_serialises_with_camel_case_and_snake_case_values() {
        let json = serde_json::to_value(current_status()).unwrap();
        assert_eq!(json["appVersion"], env!("CARGO_PKG_VERSION"));
        assert_eq!(json["computerControl"], "not_implemented");
    }
}
