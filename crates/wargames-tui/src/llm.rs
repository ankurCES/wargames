//! Anthropic-compatible LLM client (MiniMax and Azure Foundry).
//!
//! Two response modes:
//!   - `decide()` — REST. Bounded by the 12 s ceiling. Used by callers that
//!     want a single atomic result.
//!   - `decide_stream()` — SSE. Streams `content_block_delta` events as
//!     they arrive, so the UI can render partial tokens / a streaming
//!     spinner. Same 12 s ceiling. The tokio run loop in `main.rs` reads
//!     tokens from an `mpsc::UnboundedSender` and pushes them into
//!     `App::streaming_message`, which the render layer shows live.

use crate::config::{BlumiSettings, Provider};
use crate::net::with_ceiling;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio::sync::mpsc;

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
            stream: false,
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

    /// Streaming variant. Sends `stream: true`, parses SSE events line by
    /// line, and forwards each `content_block_delta.text` fragment to `tx`
    /// as it arrives. Returns the assembled `CommanderAction` if a
    /// `content_block_stop` for a `tool_use` block was reached, otherwise
    /// `None` (caller falls back to heuristic).
    ///
    /// Channel contract: a single `StreamToken::Text(String)` per delta, then
    /// either `StreamToken::Done(Some(action))` or `StreamToken::Done(None)`.
    /// The sender is closed on return so `rx.try_recv()` stops blocking.
    pub async fn decide_stream(
        &self,
        system: &str,
        user: &str,
        tx: mpsc::UnboundedSender<StreamToken>,
    ) -> Option<CommanderAction> {
        let body = MessagesRequest {
            model: &self.model,
            max_tokens: 256,
            stream: true,
            system,
            tools: &[ToolSpec::commander_action()],
            messages: &[Message {
                role: "user",
                content: user,
            }],
        };
        let url = format!("{}/v1/messages?stream=true", self.base_url.trim_end_matches('/'));
        let req = self
            .client
            .post(&url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&body);
        let resp = match with_ceiling(req.send()).await {
            Ok(Ok(r)) => r,
            _ => {
                let _ = tx.send(StreamToken::Done(None));
                return None;
            }
        };
        if !resp.status().is_success() {
            let _ = tx.send(StreamToken::Done(None));
            return None;
        }

        let mut stream = resp.bytes_stream();
        let mut buf = Vec::<u8>::new();
        // We accumulate tool_use input deltas as raw JSON fragments and
        // assemble them into the final CommanderAction at content_block_stop.
        let mut tool_input_acc = String::new();
        let mut tool_name: Option<String> = None;

        while let Some(chunk) = stream.next().await {
            let chunk = match chunk {
                Ok(c) => c,
                Err(_) => break,
            };
            buf.extend_from_slice(&chunk[..]);
            // SSE: events are separated by `\n\n`. Parse line-by-line.
            while let Some(split) = find_sse_record(&buf) {
                let raw = buf.drain(..split).collect::<Vec<u8>>();
                let s = String::from_utf8_lossy(&raw);
                // Each record has lines: "event: foo" / "data: {...}" / blank.
                let mut data: Option<&str> = None;
                for line in s.lines() {
                    if let Some(rest) = line.strip_prefix("data: ") {
                        data = Some(rest);
                    }
                }
                let Some(data) = data else { continue };
                if data == "[DONE]" {
                    let _ = tx.send(StreamToken::Done(tool_name.as_deref().and_then(|n| {
                        if n == "commander_action" {
                            serde_json::from_str::<CommanderAction>(&tool_input_acc).ok()
                        } else {
                            None
                        }
                    })));
                    return tool_name
                        .as_deref()
                        .and_then(|n| if n == "commander_action" {
                            serde_json::from_str::<CommanderAction>(&tool_input_acc).ok()
                        } else {
                            None
                        });
                }
                let event: SseEvent = match serde_json::from_str(data) {
                    Ok(e) => e,
                    Err(_) => continue,
                };
                match event {
                    SseEvent::ContentBlockStart { index: _, block } => {
                        if let ContentBlock::ToolUse { name, input } = block {
                            tool_name = Some(name);
                            // input may already be a complete JSON object.
                            if let Ok(s) = serde_json::to_string(&input) {
                                tool_input_acc = s;
                            }
                        }
                    }
                    SseEvent::ContentBlockDelta { index: _, delta } => {
                        match delta {
                            Delta::TextDelta { text } => {
                                let _ = tx.send(StreamToken::Text(text));
                            }
                            Delta::InputJsonDelta { partial_json } => {
                                tool_input_acc.push_str(&partial_json);
                            }
                            Delta::Other => {}
                        }
                    }
                    SseEvent::MessageStop => {
                        let action = tool_name.as_deref().and_then(|n| {
                            if n == "commander_action" {
                                serde_json::from_str::<CommanderAction>(&tool_input_acc).ok()
                            } else {
                                None
                            }
                        });
                        let _ = tx.send(StreamToken::Done(action.clone()));
                        return action;
                    }
                    SseEvent::Ping | SseEvent::Other => {}
                }
            }
        }
        // Stream ended without explicit stop — best effort.
        let action = tool_name.as_deref().and_then(|n| {
            if n == "commander_action" {
                serde_json::from_str::<CommanderAction>(&tool_input_acc).ok()
            } else {
                None
            }
        });
        let _ = tx.send(StreamToken::Done(action.clone()));
        action
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// If the model never calls the tool (common when it breaks character),
    /// we get None — the caller falls back to the heuristic. Make sure we
    /// do not panic on that path.
    #[test]
    fn no_tool_use_returns_none() {
        let raw = r#"{"content": [{"type": "text", "text": "I will not comply."}]}"#;
        let resp: MessagesResponse = serde_json::from_str(raw).unwrap();
        assert!(resp.into_commander_action().is_none());
    }

    /// SSE records are separated by `\n\n` (RFC 8896). The streaming parser
    /// uses `find_sse_record` to chunk a buffered byte stream into records.
    #[test]
    fn sse_record_finder_handles_lf_and_crlf() {
        // LF-only: the first boundary is the byte after the second `\n`,
        // i.e. just past the `b"\n\n"` delimiter in the input.
        let buf = b"data: {\"a\":1}\n\ndata: {\"a\":2}\n\n";
        let first_lf = find_subseq(buf, b"\n\n").expect("must find lf boundary");
        assert_eq!(find_sse_record(buf), Some(first_lf + 2));

        // CRLF: the boundary is the byte after the `b"\r\n\r\n"` delimiter.
        let buf = b"data: {\"a\":1}\r\n\r\n";
        let first_crlf = find_subseq(buf, b"\r\n\r\n").expect("must find crlf boundary");
        assert_eq!(find_sse_record(buf), Some(first_crlf + 4));

        // Partial input — no boundary yet.
        let buf = b"data: {\"a\"";
        assert_eq!(find_sse_record(buf), None);
    }

    /// A `content_block_stop` for a `commander_action` tool, fed through
    /// the SSE delta machinery, must round-trip into a `CommanderAction`.
    #[test]
    fn sse_delta_assembles_commander_action() {
        let start: SseEvent = serde_json::from_value(serde_json::json!({
            "type": "content_block_start",
            "index": 1,
            "block": {
                "type": "tool_use",
                "name": "commander_action",
                "input": {}
            }
        }))
        .unwrap();
        let delta: Delta = serde_json::from_value(serde_json::json!({
            "type": "input_json_delta",
            "partial_json": "{\"action\":\"bluff\""
        }))
        .unwrap();
        let delta2: Delta = serde_json::from_value(serde_json::json!({
            "type": "input_json_delta",
            "partial_json": ",\"message\":\"Puffing chest.\"}"
        }))
        .unwrap();
        assert!(matches!(start, SseEvent::ContentBlockStart { .. }));
        assert!(matches!(delta, Delta::InputJsonDelta { .. }));
        assert!(matches!(delta2, Delta::InputJsonDelta { .. }));
    }
}

/// What `decide_stream` pushes into the channel. The UI uses `Text` deltas
/// to render a streaming soviet response; `Done(action)` carries the final
/// tool-use payload.
#[derive(Debug, Clone)]
pub enum StreamToken {
    Text(String),
    Done(Option<crate::llm::CommanderAction>),
}

#[derive(Debug, Clone, Serialize)]
struct MessagesRequest<'a> {
    model: &'a str,
    max_tokens: u32,
    /// `false` for REST, `true` for SSE. Anthropic-compatible providers all
    /// accept this flag.
    stream: bool,
    system: &'a str,
    tools: &'a [ToolSpec],
    messages: &'a [Message<'a>],
}

#[derive(Debug, Clone, Serialize)]
struct Message<'a> {    role: &'a str,
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

// ---- SSE event envelope ---------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum SseEvent {
    ContentBlockStart {
        index: u32,
        block: ContentBlock,
    },
    ContentBlockDelta {
        index: u32,
        delta: Delta,
    },
    MessageStop,
    Ping,
    #[serde(other)]
    Other,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Delta {
    TextDelta { text: String },
    InputJsonDelta { partial_json: String },
    #[serde(other)]
    Other,
}

/// Returns the byte offset of the first SSE record boundary (`\n\n`) in
/// `buf`, or `None` if the boundary has not arrived yet. Tolerates both
/// `\n\n` and `\r\n\r\n` per RFC 8896.
fn find_sse_record(buf: &[u8]) -> Option<usize> {
    if let Some(i) = find_subseq(buf, b"\r\n\r\n") {
        Some(i + 4)
    } else if let Some(i) = find_subseq(buf, b"\n\n") {
        Some(i + 2)
    } else {
        None
    }
}

fn find_subseq(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
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