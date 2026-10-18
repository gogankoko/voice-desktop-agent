//! The tools the model may propose, and the deterministic gate every proposal passes.
//!
//! The model only ever sees `declarations()`. Anything it sends back is untrusted: the
//! name must be known, the arguments must deserialize into the exact typed shape
//! (unknown fields rejected), and the action's risk must be allowed. Nothing here is
//! decided by prompt text.

use serde::Deserialize;
use serde_json::{Value, json};

use crate::tools::Risk;
use crate::tools::open_app::AllowedApp;

/// A tool call that passed validation and may be executed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ValidatedCall {
    OpenApplication(AllowedApp),
}

impl ValidatedCall {
    pub fn risk(self) -> Risk {
        match self {
            ValidatedCall::OpenApplication(app) => app.risk(),
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OpenApplicationArgs {
    app: AllowedApp,
}

/// Function declarations in the Gemini Interactions `tools` format.
pub fn declarations() -> Vec<Value> {
    vec![json!({
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
    })]
}

/// Why a proposed call was refused. The message is sent back to the model as the tool
/// result, so it is written for the model to relay in plain language.
#[derive(Debug, PartialEq, Eq)]
pub struct Refusal(pub String);

pub fn validate(name: &str, arguments: &Value) -> Result<ValidatedCall, Refusal> {
    let call = match name {
        "open_application" => serde_json::from_value::<OpenApplicationArgs>(arguments.clone())
            .map(|a| ValidatedCall::OpenApplication(a.app))
            .map_err(|_| {
                Refusal(
                    "Invalid arguments. `app` must be one of the listed values and nothing else."
                        .into(),
                )
            })?,
        _ => {
            return Err(Refusal(format!(
                "There is no tool called `{name}`. Only use the tools you were given."
            )));
        }
    };
    if call.risk() != Risk::Low {
        // Confirmation dialogs arrive in Phase 6; until then only low-risk actions run.
        return Err(Refusal(
            "This action needs the user's explicit confirmation, which this version can't ask \
             for yet. Tell the user you can't do it yet."
                .into(),
        ));
    }
    Ok(call)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_exact_allowlisted_call() {
        assert_eq!(
            validate("open_application", &json!({"app": "default_browser"})),
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
                validate(name, &json!({"app": "default_browser"})).is_err(),
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
            assert!(validate("open_application", &args).is_err(), "{args}");
        }
    }

    #[test]
    fn declarations_match_what_validate_accepts() {
        let decl = &declarations()[0];
        assert_eq!(decl["name"], "open_application");
        let allowed: Vec<&str> = decl["parameters"]["properties"]["app"]["enum"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        for app in allowed {
            assert!(
                validate("open_application", &json!({ "app": app })).is_ok(),
                "{app}"
            );
        }
    }
}
