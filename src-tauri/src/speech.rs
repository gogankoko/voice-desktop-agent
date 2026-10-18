//! Azure AI Speech setup and short-lived token issuing.
//!
//! The resource key stays in Rust (Credential Manager). The frontend only ever receives a
//! 10-minute STS token, which is Microsoft's recommended pattern for browser-based clients.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::secrets::{self, AzureSpeechCredential};

/// Microsoft: tokens are valid for 10 minutes; reuse one for 9.
const TOKEN_REUSE: Duration = Duration::from_secs(9 * 60);
const HTTP_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpeechSetup {
    pub configured: bool,
    pub region: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpeechToken {
    pub token: String,
    pub region: String,
}

// Redacted so the token can't leak through `{:?}`.
impl std::fmt::Debug for SpeechToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SpeechToken")
            .field("token", &"<redacted>")
            .field("region", &self.region)
            .finish()
    }
}

struct CachedToken {
    token: String,
    region: String,
    issued: Instant,
}

pub struct SpeechState {
    client: reqwest::Client,
    cache: Mutex<Option<CachedToken>>,
}

impl SpeechState {
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .timeout(HTTP_TIMEOUT)
            .build()
            .expect("HTTP client");
        Self {
            client,
            cache: Mutex::new(None),
        }
    }

    fn clear_cache(&self) {
        *self.cache.lock().expect("token cache poisoned") = None;
    }
}

/// Azure region names are short lowercase identifiers such as "eastus" or "westeurope".
/// Restricting them keeps the token URL pinned to Microsoft's domain.
pub fn normalise_region(region: &str) -> Result<String, String> {
    let r = region.trim().to_ascii_lowercase();
    if (2..=40).contains(&r.len())
        && r.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
    {
        Ok(r)
    } else {
        Err("That doesn't look like an Azure region. It should be one word like \"eastus\" or \"westeurope\".".into())
    }
}

pub fn normalise_key(key: &str) -> Result<String, String> {
    let k = key.trim();
    if (20..=128).contains(&k.len()) && k.bytes().all(|b| b.is_ascii_alphanumeric()) {
        Ok(k.to_string())
    } else {
        Err("That doesn't look like an Azure Speech key. Copy \"Key 1\" from your Speech resource in the Azure portal.".into())
    }
}

fn token_url(region: &str) -> String {
    format!("https://{region}.api.cognitive.microsoft.com/sts/v1.0/issueToken")
}

/// A JWT is three base64url segments separated by dots.
fn looks_like_jwt(s: &str) -> bool {
    s.len() < 8192
        && s.split('.').count() == 3
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
}

async fn fetch_token(
    client: &reqwest::Client,
    cred: &AzureSpeechCredential,
) -> Result<String, String> {
    let resp = client
        .post(token_url(&cred.region))
        .header("Ocp-Apim-Subscription-Key", &cred.key)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .header("Content-Length", "0")
        .send()
        .await
        .map_err(|e| {
            crate::diag::log(
                "speech",
                format!("speech token request failed: {}", e.without_url()),
            );
            "I couldn't reach Microsoft's speech service. Please check the internet connection."
                .to_string()
        })?;
    let status = resp.status();
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return Err("Microsoft didn't accept that key for that region. Please check both.".into());
    }
    if !status.is_success() {
        return Err(format!(
            "Microsoft's speech service returned an error (code {}).",
            status.as_u16()
        ));
    }
    let token = resp
        .text()
        .await
        .map_err(|_| "The speech service sent an unreadable reply.".to_string())?;
    let token = token.trim().to_string();
    if !looks_like_jwt(&token) {
        return Err("The speech service sent an unexpected reply.".into());
    }
    Ok(token)
}

fn load_credential() -> Result<Option<AzureSpeechCredential>, String> {
    secrets::load_azure_speech().map_err(|e| {
        crate::diag::log("speech", format!("credential read failed: {e}"));
        "I couldn't read the saved speech settings.".to_string()
    })
}

pub fn setup() -> SpeechSetup {
    match load_credential() {
        Ok(Some(c)) => SpeechSetup {
            configured: true,
            region: Some(c.region),
        },
        _ => SpeechSetup {
            configured: false,
            region: None,
        },
    }
}

/// Validates, tests the key against Azure, and only then stores it.
pub async fn save(state: &SpeechState, region: &str, key: &str) -> Result<SpeechSetup, String> {
    let cred = AzureSpeechCredential {
        region: normalise_region(region)?,
        key: normalise_key(key)?,
    };
    let token = fetch_token(&state.client, &cred).await?;
    secrets::save_azure_speech(&cred).map_err(|e| {
        crate::diag::log("speech", format!("credential write failed: {e}"));
        "The key works, but Windows couldn't store it safely.".to_string()
    })?;
    *state.cache.lock().expect("token cache poisoned") = Some(CachedToken {
        token,
        region: cred.region.clone(),
        issued: Instant::now(),
    });
    Ok(SpeechSetup {
        configured: true,
        region: Some(cred.region),
    })
}

pub fn clear(state: &SpeechState) -> Result<SpeechSetup, String> {
    state.clear_cache();
    secrets::delete_azure_speech().map_err(|e| {
        crate::diag::log("speech", format!("credential delete failed: {e}"));
        "I couldn't remove the saved speech key.".to_string()
    })?;
    Ok(SpeechSetup {
        configured: false,
        region: None,
    })
}

pub async fn token(state: &SpeechState) -> Result<SpeechToken, String> {
    if let Some(c) = state.cache.lock().expect("token cache poisoned").as_ref() {
        if c.issued.elapsed() < TOKEN_REUSE {
            return Ok(SpeechToken {
                token: c.token.clone(),
                region: c.region.clone(),
            });
        }
    }
    let cred = load_credential()?.ok_or_else(|| {
        "Listening isn't set up yet. Add your Azure Speech key in Settings.".to_string()
    })?;
    let token = fetch_token(&state.client, &cred).await?;
    *state.cache.lock().expect("token cache poisoned") = Some(CachedToken {
        token: token.clone(),
        region: cred.region.clone(),
        issued: Instant::now(),
    });
    Ok(SpeechToken {
        token,
        region: cred.region,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regions_are_normalised_and_restricted() {
        assert_eq!(normalise_region(" EastUS ").unwrap(), "eastus");
        assert_eq!(normalise_region("westeurope").unwrap(), "westeurope");
        for bad in [
            "",
            "e",
            "east us",
            "evil.com/x",
            "eastus.attacker",
            "east-us",
            "../",
            "a".repeat(41).as_str(),
        ] {
            assert!(normalise_region(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn token_url_stays_on_microsoft_domain() {
        assert_eq!(
            token_url("eastus"),
            "https://eastus.api.cognitive.microsoft.com/sts/v1.0/issueToken"
        );
    }

    #[test]
    fn keys_are_trimmed_and_restricted() {
        let k = "a1".repeat(16);
        assert_eq!(normalise_key(&format!("  {k}\n")).unwrap(), k);
        for bad in [
            "short",
            "has spaces in the middle of it ok",
            "semi;colon000000000000000",
            &"x".repeat(129),
        ] {
            assert!(normalise_key(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn jwt_shape_check() {
        assert!(looks_like_jwt("eyJhbGciOi.eyJzdWIiOi.SflKxwRJSM-_"));
        assert!(!looks_like_jwt("<html>error</html>"));
        assert!(!looks_like_jwt("a.b"));
    }

    #[test]
    fn token_without_setup_explains_what_to_do() {
        // Only meaningful when no real credential is stored; skip otherwise.
        if setup().configured {
            return;
        }
        let state = SpeechState::new();
        let err = tauri::async_runtime::block_on(token(&state)).unwrap_err();
        assert!(err.contains("Settings"));
    }

    #[test]
    fn cached_token_is_reused_without_network() {
        let state = SpeechState::new();
        *state.cache.lock().unwrap() = Some(CachedToken {
            token: "a.b.c".into(),
            region: "eastus".into(),
            issued: Instant::now(),
        });
        let t = tauri::async_runtime::block_on(token(&state)).unwrap();
        assert_eq!((t.token.as_str(), t.region.as_str()), ("a.b.c", "eastus"));
    }
}
