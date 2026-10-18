//! The tools the model may propose, and the deterministic gate every proposal passes.
//!
//! The model only ever sees `declarations()`. Anything it sends back is untrusted: the
//! name must be known, the arguments must deserialize into the exact typed shape
//! (unknown fields rejected) and pass value checks. Whether a valid call may run at once
//! or needs the user's confirmation is decided by its `risk()`, in the orchestrator.
//! Nothing here is decided by prompt text.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::tools::Risk;
use crate::tools::open_app::AllowedApp;

const MAX_RECIPIENT_CHARS: usize = 100;
const MAX_MESSAGE_CHARS: usize = 1000;

/// A tool call that passed validation.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ValidatedCall {
    OpenApplication(AllowedApp),
    /// Practice mode only: pretends to send a message. Nothing is ever sent.
    PracticeSendMessage {
        to: String,
        message: String,
    },
}

impl ValidatedCall {
    pub fn risk(&self) -> Risk {
        match self {
            ValidatedCall::OpenApplication(app) => app.risk(),
            // Treated exactly like a real "send a message", so practice exercises the real gate.
            ValidatedCall::PracticeSendMessage { .. } => Risk::High,
        }
    }

    pub fn tool_name(&self) -> &'static str {
        match self {
            ValidatedCall::OpenApplication(_) => "open_application",
            ValidatedCall::PracticeSendMessage { .. } => "practice_send_message",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Detail {
    pub label: String,
    pub value: String,
}

/// What the confirmation screen shows. Built here from validated arguments, never from
/// the model's prose, so the user approves exactly what will run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfirmationText {
    pub action: String,
    pub details: Vec<Detail>,
    pub practice: bool,
}

pub fn confirmation_text(call: &ValidatedCall) -> ConfirmationText {
    match call {
        ValidatedCall::OpenApplication(_) => ConfirmationText {
            action: "Open your web browser".into(),
            details: vec![],
            practice: false,
        },
        ValidatedCall::PracticeSendMessage { to, message } => ConfirmationText {
            action: "Send a message (practice: nothing will really be sent)".into(),
            details: vec![
                Detail {
                    label: "To".into(),
                    value: to.clone(),
                },
                Detail {
                    label: "Message".into(),
                    value: message.clone(),
                },
            ],
            practice: true,
        },
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OpenApplicationArgs {
    app: AllowedApp,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PracticeSendMessageArgs {
    to: String,
    message: String,
}

/// Function declarations in the Gemini Interactions `tools` format.
/// The practice tool only exists while Practice mode is on.
pub fn declarations(practice: bool) -> Vec<Value> {
    let mut tools = vec![json!({
        "type": "function",
        "name": "open_application",
        "description": "Opens an application on the user's Windows computer and checks that its \
                        window came to the front. Only the applications listed in `app` exist.",
        "parameters": {
            "type": "object",
            "properties": {
                "app": {
                    "type": "string",
                    "enum": ["default_browser"],
                    "description": "default_browser: the user's web browser (for websites, the internet, searching the web)."
                }
            },
            "required": ["app"]
        }
    })];
    if practice {
        tools.push(json!({
            "type": "function",
            "name": "practice_send_message",
            "description": "PRACTICE MODE ONLY. Pretends to send a message so the user can practise \
                            approving an important action. Nothing is ever sent. Use it when the \
                            user asks to send a message or email to someone. The app itself shows \
                            the user a confirmation screen before running it; you only get the \
                            result after they have answered.",
            "parameters": {
                "type": "object",
                "properties": {
                    "to": {"type": "string", "description": "Who the message is for, e.g. a name."},
                    "message": {"type": "string", "description": "The full text of the message."}
                },
                "required": ["to", "message"]
            }
        }));
    }
    tools
}

/// Why a proposed call was refused. The message is sent back to the model as the tool
/// result, so it is written for the model to relay in plain language.
#[derive(Debug, PartialEq, Eq)]
pub struct Refusal(pub String);

fn bounded(field: &str, value: &str, max: usize) -> Result<String, Refusal> {
    let v = value.trim();
    if v.is_empty() {
        return Err(Refusal(format!("`{field}` is empty. Ask the user for it.")));
    }
    if v.chars().count() > max {
        return Err(Refusal(format!(
            "`{field}` is too long (max {max} characters)."
        )));
    }
    if v.chars().any(|c| c.is_control() && c != '\n') {
        return Err(Refusal(format!("`{field}` contains invalid characters.")));
    }
    Ok(v.to_string())
}

pub fn validate(name: &str, arguments: &Value, practice: bool) -> Result<ValidatedCall, Refusal> {
    match name {
        "open_application" => serde_json::from_value::<OpenApplicationArgs>(arguments.clone())
            .map(|a| ValidatedCall::OpenApplication(a.app))
            .map_err(|_| {
                Refusal(
                    "Invalid arguments. `app` must be one of the listed values and nothing else."
                        .into(),
                )
            }),
        "practice_send_message" if practice => {
            let a = serde_json::from_value::<PracticeSendMessageArgs>(arguments.clone()).map_err(
                |_| Refusal("Invalid arguments. Give exactly `to` and `message`.".into()),
            )?;
            Ok(ValidatedCall::PracticeSendMessage {
                to: bounded("to", &a.to, MAX_RECIPIENT_CHARS)?,
                message: bounded("message", &a.message, MAX_MESSAGE_CHARS)?,
            })
        }
        _ => Err(Refusal(format!(
            "There is no tool called `{name}`. Only use the tools you were given."
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_exact_allowlisted_call() {
        assert_eq!(
            validate(
                "open_application",
                &json!({"app": "default_browser"}),
                false
            ),
            Ok(ValidatedCall::OpenApplication(AllowedApp::DefaultBrowser))
        );
    }

    #[test]
    fn rejects_unknown_tools() {
        for name in [
            "run_shell",
            "powershell",
            "delete_file",
            "",
            "open_application ",
        ] {
            assert!(
                validate(name, &json!({"app": "default_browser"}), true).is_err(),
                "{name}"
            );
        }
    }

    #[test]
    fn rejects_anything_but_the_exact_argument_shape() {
        for args in [
            json!({}),
            json!({"app": "notepad"}),
            json!({"app": "C:\\Windows\\System32\\cmd.exe"}),
            json!({"app": "default_browser", "args": "--incognito"}),
            json!({"app": "default_browser", "url": "http://evil.example"}),
            json!({"app": ["default_browser"]}),
            json!("default_browser"),
            json!(null),
        ] {
            assert!(
                validate("open_application", &args, false).is_err(),
                "{args}"
            );
        }
    }

    #[test]
    fn practice_tool_exists_only_in_practice_mode() {
        let args = json!({"to": "Anna", "message": "Hello"});
        assert!(validate("practice_send_message", &args, false).is_err());
        assert_eq!(
            validate("practice_send_message", &args, true),
            Ok(ValidatedCall::PracticeSendMessage {
                to: "Anna".into(),
                message: "Hello".into()
            })
        );
        assert_eq!(declarations(false).len(), 1);
        assert_eq!(declarations(true).len(), 2);
    }

    #[test]
    fn practice_tool_is_high_risk_and_checks_values() {
        let ok = validate(
            "practice_send_message",
            &json!({"to": " Anna ", "message": "Hi"}),
            true,
        )
        .unwrap();
        assert_eq!(ok.risk(), Risk::High);
        for bad in [
            json!({"to": "", "message": "Hi"}),
            json!({"to": "Anna"}),
            json!({"to": "Anna", "message": "Hi", "cc": "Bob"}),
            json!({"to": "x".repeat(101), "message": "Hi"}),
            json!({"to": "Anna", "message": "x".repeat(1001)}),
            json!({"to": "An\u{0007}na", "message": "Hi"}),
        ] {
            assert!(
                validate("practice_send_message", &bad, true).is_err(),
                "{bad}"
            );
        }
    }

    #[test]
    fn confirmation_text_comes_from_validated_values() {
        let call = ValidatedCall::PracticeSendMessage {
            to: "Anna".into(),
            message: "See you at 5".into(),
        };
        let t = confirmation_text(&call);
        assert!(t.practice);
        assert_eq!(
            t.details[0],
            Detail {
                label: "To".into(),
                value: "Anna".into()
            }
        );
        assert_eq!(t.details[1].value, "See you at 5");
    }

    #[test]
    fn declarations_match_what_validate_accepts() {
        let decl = &declarations(false)[0];
        assert_eq!(decl["name"], "open_application");
        let allowed: Vec<&str> = decl["parameters"]["properties"]["app"]["enum"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        for app in allowed {
            assert!(
                validate("open_application", &json!({ "app": app }), false).is_ok(),
                "{app}"
            );
        }
    }
}
