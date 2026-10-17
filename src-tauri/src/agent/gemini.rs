//! Google Gemini via the Interactions API (`POST /v1beta/interactions`), stateless:
//! `store: false`, so Google keeps nothing between requests and we resend the
//! conversation each time. Shapes follow Google's published OpenAPI spec (2026-10).

use std::future::Future;
use std::time::Duration;

use serde_json::{Value, json};

use super::prompt::SYSTEM_INSTRUCTION;
use super::registry;

/// Pinned rather than a `-latest` alias so behaviour doesn't change underneath us.
pub const MODEL: &str = "gemini-3.8-flash";
const ENDPOINT: &str = "https://generativelanguage.googleapis.com/v1beta/interactions";
const MODELS_ENDPOINT: &str = "https://generativelanguage.googleapis.com/v1beta/models?pageSize=1";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// A function call proposed by the model. Untrusted until `registry::validate` accepts it.
#[derive(Debug, Clone, PartialEq)]
pub struct ProposedCall {
    pub id: String,
    pub name: String,
    pub arguments: Value,
}

#[derive(Debug, Clone, Default)]
pub struct ModelTurn {
    /// Every step the model returned, unmodified, for replay (keeps thought signatures valid).
    pub steps: Vec<Value>,
    pub calls: Vec<ProposedCall>,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelError {
    Auth,
    RateLimited,
    Network,
    Server(u16),
    BadResponse,
}

impl ModelError {
    pub fn user_message(&self) -> String {
        match self {
            ModelError::Auth => "Google didn't accept the assistant key. Please check it in Settings.",
            ModelError::RateLimited => {
                "The assistant is busy, or today's free limit has been reached. Please wait a minute and try again."
            }
            ModelError::Network => {
                "I couldn't reach the assistant service. Please check the internet connection."
            }
            ModelError::Server(_) => "The assistant service had a problem. Please try again in a moment.",
            ModelError::BadResponse => {
                "The assistant sent a reply I couldn't understand. Please try again."
            }
        }
        .to_string()
    }
}

/// Provider-independent seam: the orchestrator only needs this.
pub trait ChatModel {
    /// `input` is the full conversation as Interactions steps.
    fn respond(
        &self,
        input: &[Value],
    ) -> impl Future<Output = Result<ModelTurn, ModelError>> + Send;
}

pub struct Gemini {
    http: reqwest::Client,
    key: String,
}

impl Gemini {
    pub fn new(http: reqwest::Client, key: String) -> Self {
        Self { http, key }
    }
}

pub fn request_body(input: &[Value]) -> Value {
    json!({
        "model": MODEL,
        "input": input,
        "system_instruction": SYSTEM_INSTRUCTION,
        "tools": registry::declarations(),
        "store": false,
        "generation_config": {
            "thinking_level": "low",
            "max_output_tokens": 1024
        }
    })
}

impl ChatModel for Gemini {
    async fn respond(&self, input: &[Value]) -> Result<ModelTurn, ModelError> {
        let resp = self
            .http
            .post(ENDPOINT)
            .header("x-goog-api-key", &self.key)
            .timeout(REQUEST_TIMEOUT)
            .header("Content-Type", "application/json")
            .body(request_body(input).to_string())
            .send()
            .await
            .map_err(|e| {
                eprintln!("gemini request failed: {}", e.without_url());
                ModelError::Network
            })?;
        let status = resp.status();
        if !status.is_success() {
            eprintln!("gemini returned HTTP {}", status.as_u16());
            return Err(match status.as_u16() {
                401 | 403 => ModelError::Auth,
                400 if is_key_problem(resp).await => ModelError::Auth,
                429 => ModelError::RateLimited,
                s if s >= 500 => ModelError::Server(s),
                _ => ModelError::BadResponse,
            });
        }
        let bytes = resp.bytes().await.map_err(|_| ModelError::Network)?;
        let body: Value = serde_json::from_slice(&bytes).map_err(|_| ModelError::BadResponse)?;
        parse_interaction(&body)
    }
}

/// Google reports an invalid key as HTTP 400 with reason API_KEY_INVALID.
async fn is_key_problem(resp: reqwest::Response) -> bool {
    resp.text()
        .await
        .map(|t| t.contains("API_KEY_INVALID") || t.contains("API key not valid"))
        .unwrap_or(false)
}

pub fn parse_interaction(body: &Value) -> Result<ModelTurn, ModelError> {
    if body["status"] == "failed" {
        eprintln!("gemini interaction failed: {}", body["errors"]);
        return Err(ModelError::BadResponse);
    }
    let mut turn = ModelTurn::default();
    for step in body["steps"].as_array().into_iter().flatten() {
        match step["type"].as_str() {
            Some("function_call") => {
                let (Some(id), Some(name)) = (step["id"].as_str(), step["name"].as_str()) else {
                    return Err(ModelError::BadResponse);
                };
                turn.calls.push(ProposedCall {
                    id: id.to_string(),
                    name: name.to_string(),
                    arguments: step["arguments"].clone(),
                });
            }
            Some("model_output") => {
                for block in step["content"].as_array().into_iter().flatten() {
                    if block["type"] == "text" {
                        if let Some(t) = block["text"].as_str() {
                            if !turn.text.is_empty() {
                                turn.text.push(' ');
                            }
                            turn.text.push_str(t.trim());
                        }
                    }
                }
            }
            _ => {} // thought and other steps: kept for replay only
        }
        turn.steps.push(step.clone());
    }
    if turn.calls.is_empty() && turn.text.is_empty() {
        return Err(ModelError::BadResponse);
    }
    Ok(turn)
}

/// Light sanity check before asking Google. Google's own check decides validity; this only
/// catches copy-paste accidents and anything unsafe to put in an HTTP header.
/// Error messages describe the problem without echoing the key.
pub fn normalise_key(raw: &str) -> Result<String, String> {
    let key = raw
        .trim()
        .trim_matches(|c| c == '"' || c == '\'' || c == '`');
    if key.is_empty() {
        return Err("Please paste the key into the box first.".into());
    }
    if key.chars().any(char::is_whitespace) {
        return Err(
            "The key has a space or line break in the middle. Please copy it again as one piece."
                .into(),
        );
    }
    if !key.bytes().all(|b| b.is_ascii_graphic()) {
        return Err("The key contains unusual characters. Please copy it again from Google AI Studio using its copy button.".into());
    }
    if key.len() < 20 {
        return Err(format!(
            "The key looks too short ({} characters), so it may not have copied completely. Use the copy button in Google AI Studio.",
            key.len()
        ));
    }
    if key.len() > 512 {
        return Err("That's much longer than a key. Please copy just the key itself.".into());
    }
    Ok(key.to_string())
}

/// Checks a key with a free metadata call (no tokens used).
pub async fn test_key(http: &reqwest::Client, key: &str) -> Result<(), String> {
    let resp = http
        .get(MODELS_ENDPOINT)
        .header("x-goog-api-key", key)
        .timeout(REQUEST_TIMEOUT)
        .send()
        .await
        .map_err(|e| {
            eprintln!("gemini key test failed: {}", e.without_url());
            "I couldn't reach Google. Please check the internet connection.".to_string()
        })?;
    match resp.status().as_u16() {
        200 => Ok(()),
        400 | 401 | 403 => {
            Err("Google didn't accept that key. Please copy it again from Google AI Studio.".into())
        }
        429 => Err(
            "Google says the key is rate-limited right now. Please try again in a minute.".into(),
        ),
        s => Err(format!(
            "Google returned an error (code {s}). Please try again later."
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_function_call_and_keeps_steps_verbatim() {
        let body = json!({
            "id": "i1", "status": "requires_action",
            "steps": [
                {"type": "thought", "signature": "c2lnbmF0dXJl"},
                {"type": "function_call", "id": "call_1", "name": "open_application",
                 "arguments": {"app": "default_browser"}}
            ]
        });
        let turn = parse_interaction(&body).unwrap();
        assert_eq!(
            turn.calls,
            vec![ProposedCall {
                id: "call_1".into(),
                name: "open_application".into(),
                arguments: json!({"app": "default_browser"}),
            }]
        );
        assert_eq!(turn.steps, body["steps"].as_array().unwrap().clone());
    }

    #[test]
    fn parses_text_output() {
        let body = json!({"status": "completed", "steps": [
            {"type": "model_output", "content": [{"type": "text", "text": " Hello there. "}]}
        ]});
        assert_eq!(parse_interaction(&body).unwrap().text, "Hello there.");
    }

    #[test]
    fn rejects_failed_empty_and_malformed() {
        assert_eq!(
            parse_interaction(&json!({"status": "failed", "errors": []})).unwrap_err(),
            ModelError::BadResponse
        );
        assert_eq!(
            parse_interaction(&json!({"status": "completed", "steps": []})).unwrap_err(),
            ModelError::BadResponse
        );
        assert_eq!(
            parse_interaction(&json!({"status": "completed"})).unwrap_err(),
            ModelError::BadResponse
        );
        let no_id = json!({"steps": [{"type": "function_call", "name": "open_application", "arguments": {}}]});
        assert_eq!(
            parse_interaction(&no_id).unwrap_err(),
            ModelError::BadResponse
        );
    }

    #[test]
    fn request_is_stateless_and_pinned() {
        let body = request_body(&[
            json!({"type": "user_input", "content": [{"type": "text", "text": "hi"}]}),
        ]);
        assert_eq!(body["store"], false);
        assert_eq!(body["model"], MODEL);
        assert!(body.get("previous_interaction_id").is_none());
        assert_eq!(body["tools"][0]["name"], "open_application");
    }

    #[test]
    fn key_check_accepts_real_formats_and_explains_problems() {
        let classic = format!("AIza{}", "x".repeat(35));
        assert_eq!(
            normalise_key(&format!(
                "  {classic}
"
            ))
            .unwrap(),
            classic
        );
        assert_eq!(normalise_key(&format!("\"{classic}\"")).unwrap(), classic);
        // Other Google formats (e.g. with dots) must not be rejected by us.
        let dotted = format!("AQ.{}", "Ab8_-x".repeat(10));
        assert_eq!(normalise_key(&dotted).unwrap(), dotted);

        assert!(normalise_key("").unwrap_err().contains("paste"));
        assert!(
            normalise_key("AIza abc defghijklmnopqrstuvwxyz")
                .unwrap_err()
                .contains("space")
        );
        assert!(
            normalise_key(&format!("AIza{}é", "x".repeat(30)))
                .unwrap_err()
                .contains("unusual")
        );
        assert!(
            normalise_key("AIzaShort")
                .unwrap_err()
                .contains("too short")
        );
        // Never echo the key back.
        let err = normalise_key("AIzaSecretButShort").unwrap_err();
        assert!(!err.contains("AIzaSecret"));
    }

    #[test]
    fn error_messages_are_plain_language() {
        for e in [
            ModelError::Auth,
            ModelError::RateLimited,
            ModelError::Network,
            ModelError::Server(503),
            ModelError::BadResponse,
        ] {
            let m = e.user_message();
            assert!(
                !m.contains("HTTP") && !m.contains("API_KEY") && m.ends_with('.'),
                "{m}"
            );
        }
    }
}
