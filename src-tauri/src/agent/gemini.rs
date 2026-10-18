//! Google Gemini via the Interactions API (`POST /v1beta/interactions`), stateless:
//! `store: false`, so Google keeps nothing between requests and we resend the
//! conversation each time. Shapes follow Google's published OpenAPI spec (2026-10).

use std::future::Future;
use std::sync::Mutex;
use std::time::Duration;

use serde_json::{Value, json};

use super::prompt::{date_line, system_instruction};
use super::registry;

/// Pinned rather than a `-latest` alias so behaviour doesn't change underneath us.
pub const MODEL: &str = "gemini-3.8-flash";
/// Tried in order when the primary model is overloaded or rate-limited at the start of a
/// request. Each has its own free-tier quota; the first is nearly as capable as the primary,
/// the last is cheaper and weaker (it tends to stop after the first step of a multi-step task).
pub const FALLBACK_MODELS: [&str; 2] = ["gemini-3.6-flash", "gemini-3.5-flash-lite"];
#[cfg(test)]
pub const FALLBACK_MODEL: &str = FALLBACK_MODELS[0];
const RETRY_DELAY: Duration = Duration::from_millis(1500);
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
    /// HTTP 503: Google says the model is under high demand.
    Overloaded,
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
            ModelError::Overloaded => {
                "Google's assistant service is very busy right now. Please try again in a minute."
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

    /// The model currently in use (it can change once, via fallback, at the start of a request).
    fn model_id(&self) -> &'static str;
}

/// One `Gemini` per user request: once a request falls back to another model, it stays
/// on that model, because thought signatures are tied to the model that produced them.
pub struct Gemini {
    http: reqwest::Client,
    key: String,
    model: Mutex<&'static str>,
    /// Practice mode: the pretend high-risk tool is offered to the model.
    practice: bool,
}

impl Gemini {
    pub fn new(http: reqwest::Client, key: String, practice: bool) -> Self {
        Self::resume(http, key, practice, MODEL)
    }

    /// Continues a paused request on the model that produced its steps.
    pub fn resume(http: reqwest::Client, key: String, practice: bool, model: &'static str) -> Self {
        Self {
            http,
            key,
            model: Mutex::new(model),
            practice,
        }
    }

    async fn send(&self, model: &'static str, input: &[Value]) -> Result<ModelTurn, ModelError> {
        let resp = self
            .http
            .post(ENDPOINT)
            .header("x-goog-api-key", &self.key)
            .timeout(REQUEST_TIMEOUT)
            .header("Content-Type", "application/json")
            .body(
                request_body(model, input, self.practice, &crate::tools::win::local_now())
                    .to_string(),
            )
            .send()
            .await
            .map_err(|e| {
                crate::diag::log(
                    "gemini",
                    format!("gemini request failed: {}", e.without_url()),
                );
                ModelError::Network
            })?;
        let status = resp.status();
        if !status.is_success() {
            crate::diag::log(
                "gemini",
                format!("gemini ({model}) returned HTTP {}", status.as_u16()),
            );
            return Err(match status.as_u16() {
                401 | 403 => ModelError::Auth,
                400 if is_key_problem(resp).await => ModelError::Auth,
                429 => ModelError::RateLimited,
                503 => ModelError::Overloaded,
                s if s >= 500 => ModelError::Server(s),
                _ => ModelError::BadResponse,
            });
        }
        let bytes = resp.bytes().await.map_err(|_| ModelError::Network)?;
        let body: Value = serde_json::from_slice(&bytes).map_err(|_| ModelError::BadResponse)?;
        parse_interaction(&body)
    }
}

/// True once the conversation contains steps tied to a specific model (thoughts, calls,
/// results). Switching model is only safe before that.
fn has_model_bound_steps(input: &[Value]) -> bool {
    input.iter().any(|s| {
        matches!(
            s["type"].as_str(),
            Some("thought" | "function_call" | "function_result")
        )
    })
}

/// Bounded retry policy (returns the result and the model that produced it):
/// - transient server errors: one retry after `delay`;
/// - fresh request (no model-bound steps yet) and the model is overloaded or rate-limited:
///   try each fallback model in turn;
/// - mid-request rate limit: the model can't change, so wait (3x then 6x `delay`) and retry,
///   instead of abandoning a half-done task.
pub async fn with_retry_and_fallback<F, Fut>(
    current: &'static str,
    fresh: bool,
    delay: Duration,
    mut attempt: F,
) -> (Result<ModelTurn, ModelError>, &'static str)
where
    F: FnMut(&'static str) -> Fut,
    Fut: Future<Output = Result<ModelTurn, ModelError>>,
{
    let unavailable = |r: &Result<ModelTurn, ModelError>| {
        matches!(r, Err(ModelError::Overloaded | ModelError::RateLimited))
    };
    let mut result = attempt(current).await;
    if matches!(result, Err(ModelError::Overloaded | ModelError::Server(_))) {
        tokio::time::sleep(delay).await;
        result = attempt(current).await;
    }
    if fresh && unavailable(&result) {
        let mut used = current;
        for fallback in FALLBACK_MODELS.into_iter().filter(|m| *m != current) {
            crate::diag::log(
                "gemini",
                format!("gemini: {used} unavailable, trying {fallback}"),
            );
            used = fallback;
            result = attempt(fallback).await;
            if !unavailable(&result) {
                break;
            }
        }
        return (result, used);
    }
    if !fresh {
        for factor in [3, 6] {
            if !matches!(result, Err(ModelError::RateLimited)) {
                break;
            }
            crate::diag::log(
                "gemini",
                format!("gemini: {current} rate-limited mid-request, waiting"),
            );
            tokio::time::sleep(delay * factor).await;
            result = attempt(current).await;
        }
    }
    (result, current)
}

pub fn request_body(model: &str, input: &[Value], practice: bool, now: &str) -> Value {
    json!({
        "model": model,
        "input": input,
        "system_instruction": format!("{}{}", system_instruction(practice), date_line(now)),
        "tools": registry::declarations(practice),
        "store": false,
        "generation_config": {
            "thinking_level": "low",
            "max_output_tokens": 1024
        }
    })
}

impl ChatModel for Gemini {
    async fn respond(&self, input: &[Value]) -> Result<ModelTurn, ModelError> {
        let current = *self.model.lock().expect("model lock");
        let fresh = !has_model_bound_steps(input);
        let (result, used) =
            with_retry_and_fallback(current, fresh, RETRY_DELAY, |m| self.send(m, input)).await;
        if result.is_ok() && used != current {
            *self.model.lock().expect("model lock") = used;
        }
        result
    }

    fn model_id(&self) -> &'static str {
        *self.model.lock().expect("model lock")
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
        crate::diag::log(
            "gemini",
            format!("gemini interaction failed: {}", body["errors"]),
        );
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
            crate::diag::log(
                "gemini",
                format!("gemini key test failed: {}", e.without_url()),
            );
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
        let body = request_body(
            MODEL,
            &[json!({"type": "user_input", "content": [{"type": "text", "text": "hi"}]})],
            false,
            "Sunday 18 October 2026, 14:05",
        );
        assert!(
            body["system_instruction"]
                .as_str()
                .unwrap()
                .contains("18 October 2026")
        );
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

    fn ok_turn() -> Result<ModelTurn, ModelError> {
        Ok(ModelTurn {
            text: "hi".into(),
            ..Default::default()
        })
    }

    type PolicyRun = (
        Result<ModelTurn, ModelError>,
        &'static str,
        Vec<&'static str>,
    );

    /// Runs the policy with scripted results per attempt; returns (result, model used, models tried).
    fn policy(fresh: bool, script: Vec<Result<ModelTurn, ModelError>>) -> PolicyRun {
        let script = Mutex::new(std::collections::VecDeque::from(script));
        let tried = Mutex::new(vec![]);
        let (r, used) = tauri::async_runtime::block_on(with_retry_and_fallback(
            MODEL,
            fresh,
            Duration::ZERO,
            |m| {
                tried.lock().unwrap().push(m);
                let next = script
                    .lock()
                    .unwrap()
                    .pop_front()
                    .expect("script exhausted");
                async move { next }
            },
        ));
        (r, used, tried.into_inner().unwrap())
    }

    #[test]
    fn success_needs_no_retry() {
        let (r, used, tried) = policy(true, vec![ok_turn()]);
        assert!(r.is_ok());
        assert_eq!((used, tried), (MODEL, vec![MODEL]));
    }

    #[test]
    fn overloaded_retries_once_then_falls_back_when_fresh() {
        let (r, used, tried) = policy(
            true,
            vec![
                Err(ModelError::Overloaded),
                Err(ModelError::Overloaded),
                ok_turn(),
            ],
        );
        assert!(r.is_ok());
        assert_eq!(used, FALLBACK_MODEL);
        assert_eq!(tried, vec![MODEL, MODEL, FALLBACK_MODEL]);
    }

    #[test]
    fn transient_error_recovers_on_retry() {
        let (r, used, tried) = policy(true, vec![Err(ModelError::Overloaded), ok_turn()]);
        assert!(r.is_ok());
        assert_eq!((used, tried.len()), (MODEL, 2));
    }

    #[test]
    fn rate_limit_goes_straight_to_fallback() {
        let (r, used, tried) = policy(true, vec![Err(ModelError::RateLimited), ok_turn()]);
        assert!(r.is_ok());
        assert_eq!(used, FALLBACK_MODEL);
        assert_eq!(tried, vec![MODEL, FALLBACK_MODEL]);
    }

    #[test]
    fn never_switches_model_mid_request() {
        let (r, used, tried) = policy(
            false,
            vec![Err(ModelError::Overloaded), Err(ModelError::Overloaded)],
        );
        assert_eq!(r.unwrap_err(), ModelError::Overloaded);
        assert_eq!((used, tried), (MODEL, vec![MODEL, MODEL]));
    }

    #[test]
    fn auth_and_bad_responses_are_not_retried() {
        for e in [ModelError::Auth, ModelError::BadResponse] {
            let (r, _, tried) = policy(true, vec![Err(e.clone())]);
            assert_eq!(r.unwrap_err(), e);
            assert_eq!(tried.len(), 1);
        }
    }

    #[test]
    fn fallback_chain_tries_the_next_model() {
        let (r, used, tried) = policy(
            true,
            vec![
                Err(ModelError::RateLimited),
                Err(ModelError::RateLimited),
                ok_turn(),
            ],
        );
        assert!(r.is_ok());
        assert_eq!(used, FALLBACK_MODELS[1]);
        assert_eq!(tried, vec![MODEL, FALLBACK_MODELS[0], FALLBACK_MODELS[1]]);
    }

    #[test]
    fn mid_request_rate_limit_waits_and_keeps_the_same_model() {
        let (r, used, tried) = policy(false, vec![Err(ModelError::RateLimited), ok_turn()]);
        assert!(r.is_ok());
        assert_eq!((used, tried), (MODEL, vec![MODEL, MODEL]));
        let (r, _, tried) = policy(
            false,
            vec![
                Err(ModelError::RateLimited),
                Err(ModelError::RateLimited),
                Err(ModelError::RateLimited),
            ],
        );
        assert_eq!(r.unwrap_err(), ModelError::RateLimited);
        assert_eq!(tried.len(), 3, "bounded: two waits, then give up");
    }

    #[test]
    fn model_bound_steps_are_detected() {
        let user = json!({"type": "user_input", "content": []});
        let text = json!({"type": "model_output", "content": []});
        assert!(!has_model_bound_steps(&[user.clone(), text]));
        assert!(has_model_bound_steps(&[
            user,
            json!({"type": "thought", "signature": "x"})
        ]));
    }

    /// Live diagnostic against Google with the key stored in Credential Manager.
    /// Prints only HTTP status and Google's response (never the key).
    /// Run: cargo test --manifest-path src-tauri/Cargo.toml live_gemini -- --ignored --nocapture
    #[test]
    #[ignore = "live call using the owner's stored Gemini key"]
    fn live_gemini_request_variants() {
        let key = crate::secrets::load_gemini_key()
            .unwrap()
            .expect("a Gemini key is stored");
        let http = reqwest::Client::new();
        let user = json!([{"type": "user_input", "content": [{"type": "text", "text": "Say hello in five words."}]}]);
        let full = request_body(MODEL, user.as_array().unwrap(), false, "test");
        let mut variants: Vec<(&str, Value)> = vec![
            (
                "minimal (model + string input)",
                json!({"model": MODEL, "input": "Say hello in five words."}),
            ),
            (
                "+ steps input",
                json!({"model": MODEL, "input": user, "store": false}),
            ),
            (
                "+ store:false",
                json!({"model": MODEL, "input": user, "store": false}),
            ),
            (
                "+ system_instruction",
                json!({"model": MODEL, "input": user, "store": false, "system_instruction": system_instruction(false)}),
            ),
            (
                "+ tools",
                json!({"model": MODEL, "input": user, "store": false, "tools": registry::declarations(false)}),
            ),
            (
                "+ generation_config",
                json!({"model": MODEL, "input": user, "store": false, "generation_config": full["generation_config"]}),
            ),
        ];
        variants.push(("full app request", full));
        variants.push((
            "full request on fallback model",
            request_body(FALLBACK_MODEL, user.as_array().unwrap(), false, "test"),
        ));
        tauri::async_runtime::block_on(async {
            for (label, body) in variants {
                let resp = http
                    .post(ENDPOINT)
                    .header("x-goog-api-key", &key)
                    .header("Content-Type", "application/json")
                    .body(body.to_string())
                    .send()
                    .await;
                match resp {
                    Err(e) => println!("[{label}] network error: {}", e.without_url()),
                    Ok(r) => {
                        let status = r.status().as_u16();
                        let text = r.text().await.unwrap_or_default();
                        let short: String = text.chars().take(600).collect();
                        println!(
                            "[{label}] HTTP {status}: {short}
"
                        );
                    }
                }
            }
        });
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
