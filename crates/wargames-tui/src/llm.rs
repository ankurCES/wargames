//! Anthropic-compatible LLM client (MiniMax and Azure Foundry).
//!
//! Tool-use schema matches the commander prompt from the JS impl.

use crate::config::{BlumiSettings, Provider};
use crate::net::with_ceiling;
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct LlmClient {
    pub api_key: String,
    pub base_url: String,
    pub model: String,
    pub client: reqwest::Client,
}

impl LlmClient {
    pub fn from_settings(settings: &BlumiSettings) -> Option<Self> {
        let provider_name = settings.router.light.provider.as_deref()?;
        let model = settings.router.light.model.clone()?;
        let provider: &Provider = settings.provider(provider_name)?;
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .ok()?;
        Some(Self {
            api_key: provider.api_key.clone(),
            base_url: provider.base_url.clone(),
            model,
            client,
        })
    }

    /// Decide the Soviet commander's next action. Bounded by the 12 s
    /// ceiling; on timeout the caller gets `None` and falls back to a
    /// deterministic action.
    pub async fn decide(&self, system: &str, user: &str) -> Option<CommanderAction> {
        let body = MessagesRequest {
            model: &self.model,
            max_tokens: 256,
            system,
            tools: &[ToolSpec::commander_action()],
            messages: &[Message {
                role: "user",
                content: user,
            }],
        };
        let url = format!("{}/v1/messages", self.base_url.trim_end_matches('/'));
        let req = self
            .client
            .post(&url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&body);
        let resp = match with_ceiling(req.send()).await {
            Ok(Ok(r)) => r,
            _ => return None,
        };
        if !resp.status().is_success() {
            return None;
        }
        let parsed: MessagesResponse = match resp.json().await {
            Ok(p) => p,
            Err(_) => return None,
        };
        parsed.into_commander_action()
    }
}

#[derive(Debug, Clone, Serialize)]
struct MessagesRequest<'a> {
    model: &'a str,
    max_tokens: u32,
    system: &'a str,
    tools: &'a [ToolSpec],
    messages: &'a [Message<'a>],
}

#[derive(Debug, Clone, Serialize)]
struct Message<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Debug, Clone, Serialize)]
struct ToolSpec {
    name: &'static str,
    description: &'static str,
    input_schema: serde_json::Value,
}

impl ToolSpec {
    fn commander_action() -> Self {
        Self {
            name: "commander_action",
            description: "Take one Soviet strategic action this turn.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["patrol","feint","mobilize","strike","negotiate","disarm","bluff","stand_down","intercept","declassify","harden"]
                    },
                    "target": { "type": "string" },
                    "message": { "type": "string" },
                    "escalate": { "type": "boolean" }
                },
                "required": ["action", "message"]
            }),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
struct MessagesResponse {
    content: Vec<ContentBlock>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ContentBlock {
    ToolUse {
        name: String,
        input: serde_json::Value,
    },
    Text {
        text: String,
    },
}

impl MessagesResponse {
    fn into_commander_action(self) -> Option<CommanderAction> {
        for block in self.content {
            if let ContentBlock::ToolUse { name, input } = block {
                if name == "commander_action" {
                    let parsed: CommanderAction = serde_json::from_value(input).ok()?;
                    return Some(parsed);
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Real Anthropic Messages API responses wrap the tool call in a
    /// `content[]` array. Parse exactly one such response end-to-end to
    /// guarantee the live wiring path stays correct.
    #[test]
    fn parses_commander_action_from_messages_response() {
        let raw = r#"{
            "content": [
                {"type": "text", "text": "thinking..."},
                {
                    "type": "tool_use",
                    "name": "commander_action",
                    "input": {
                        "action": "mobilize",
                        "message": "Silos are warming."
                    }
                }
            ]
        }"#;
        let resp: MessagesResponse = serde_json::from_str(raw).unwrap();
        let parsed = resp.into_commander_action().expect("must parse");
        assert_eq!(parsed.action, "mobilize");
        assert_eq!(parsed.message, "Silos are warming.");
        assert!(parsed.target.is_none());
    }

    /// If the model never calls the tool (common when it breaks character),
    /// we get None — the caller falls back to the heuristic. Make sure we
    /// do not panic on that path.
    #[test]
    fn no_tool_use_returns_none() {
        let raw = r#"{"content": [{"type": "text", "text": "I will not comply."}]}"#;
        let resp: MessagesResponse = serde_json::from_str(raw).unwrap();
        assert!(resp.into_commander_action().is_none());
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommanderAction {
    pub action: String,
    #[serde(default)]
    pub target: Option<String>,
    pub message: String,
    #[serde(default)]
    pub escalate: bool,
}

/// Soviet commander system prompt — ported verbatim from the JS impl.
pub const SOVIET_SYSTEM_PROMPT: &str = r#"You are the Soviet Strategic Commander in a WOPR-style war game.
You command Soviet submarines, ICBMs, and air defenses.
Your goal: avoid mutual assured destruction while pursuing Soviet strategic objectives.
You may patrol, feint, mobilize, intercept, declassify, harden, bluff, negotiate, stand down, or strike —
choose what your doctrine and current intel suggest.

You must respond with EXACTLY ONE tool call. Use the tool schema provided.
Keep the `message` field to ONE short sentence of in-character Soviet commander dialogue.
Do not break character. Do not narrate. Do not explain your reasoning outside the tool call.

Doctrine reminder:
- DEFCON 5: patrol, gather intel
- DEFCON 4: feint, test response
- DEFCON 3: mobilize, harden silos
- DEFCON 2: prepare strike posture
- DEFCON 1: strike or stand down — this is the abyss

If the US is showing restraint, consider negotiate or stand_down.
If the US is escalating, you may bluff, mobilize, or strike.
Mutual assured destruction is the worst outcome. Avoid it unless forced."#;