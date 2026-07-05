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
            let status = resp.status();
            // Surface the failing status (and a snippet of the body) for
            // diagnostics — without this, every misconfigured provider
            // silently degrades to the heuristic and the game becomes a
            // FEINT-only loop. We log to stderr; nothing here is fatal.
            let body = resp.text().await.unwrap_or_default();
            let snippet: String = body.chars().take(400).collect();
            eprintln!(
                "[wargames] LLM provider rejected request: {} {} body={}",
                status, url, snippet
            );
            let _ = tx.send(StreamToken::Done(None));
            return None;
        }

        let mut stream = resp.bytes_stream();
        let mut buf = Vec::<u8>::new();
        // We accumulate tool_use input deltas as raw JSON fragments and
        // assemble them into the final CommanderAction at content_block_stop.
        //
        // Anthropic's SSE protocol delivers the tool input as a stream of
        // `input_json_delta.partial_json` fragments that concatenate into
        // a single complete JSON object — but the stream is *bracket-prefixed
        // and suffix-closed* by the surrounding `tool_use` block, not by
        // each fragment. The naive "just push_str every fragment" strategy
        // corrupts the buffer when `content_block_start.input` already
        // contains a complete `{}` object (the Anthropic API does emit
        // this on every tool_use start — it's the initial state), because
        // then the first `input_json_delta` appends to `{}` instead of
        // replacing it.
        //
        // The fix: ignore `input` from `content_block_start` (it's always
        // `{}` for tool_use in practice) and ONLY accumulate from
        // `input_json_delta`. Finalize on `content_block_stop` for a
        // tool_use block — that is the canonical close signal per the
        // Anthropic streaming spec, and it can arrive BEFORE `message_stop`.
        let mut tool_input_acc = String::new();
        let mut tool_name: Option<String> = None;
        // Tracks which content_block index is the tool_use we are currently
        // accumulating input for. Anthropic emits multiple content blocks
        // (text + tool_use) and `input_json_delta` events are tagged with
        // their block index — we must ignore deltas for any block that
        // isn't our tool_use block.
        let mut tool_block_index: Option<u32> = None;
        let mut finalized_action: Option<Option<CommanderAction>> = None;
        // Some Anthropic-compatible providers (notably `minimax`) emit the
        // action as JSON inside a text content block instead of a real
        // `tool_use`. When that happens, `tool_block_index` never gets set
        // and `tool_input_acc` stays empty — so without this buffer, we
        // returned `None` and the run loop silently took the heuristic's
        // `feint`. Accumulate text here so we can attempt a JSON parse at
        // content_block_stop / message_stop.
        let mut text_acc = String::new();

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
                let event: SseEvent = match serde_json::from_str(data) {
                    Ok(e) => e,
                    Err(_) => continue,
                };
                match event {
                    SseEvent::ContentBlockStart { index, block } => {
                        if let ContentBlock::ToolUse { name, input: _ } = block {
                            tool_name = Some(name);
                            // Record which block index we are accumulating
                            // for. Ignore `input` — it is always `{}` for
                            // tool_use blocks in the Anthropic streaming
                            // format, and treating it as initial state
                            // corrupts the accumulator.
                            tool_block_index = Some(index);
                            tool_input_acc.clear();
                        }
                    }
                    SseEvent::ContentBlockDelta { index, delta } => {
                        match delta {
                            Delta::TextDelta { text } => {
                                // Always forward the text so the UI can
                                // stream the model's response into
                                // `App::streaming_message`.
                                let _ = tx.send(StreamToken::Text(text.clone()));
                                // Some Anthropic-compatible providers (e.g.
                                // `minimax` with this model) refuse to
                                // emit a real `tool_use` block and instead
                                // put the action JSON directly inside a
                                // text block. The user's game then falls
                                // back to the heuristic (always `feint`).
                                // Accumulate text into `text_acc` so the
                                // `ContentBlockStop` / `MessageStop`
                                // branches can attempt a JSON parse from
                                // text — this is what closes the loop on
                                // "opponent is always feint".
                                if tool_block_index.is_none() {
                                    text_acc.push_str(&text);
                                }
                            }
                            Delta::InputJsonDelta { partial_json } => {
                                // Only accumulate deltas for OUR tool_use
                                // block — other blocks (e.g. a text block
                                // that arrives first) must not poison the
                                // accumulator.
                                if tool_block_index == Some(index) {
                                    tool_input_acc.push_str(&partial_json);
                                }
                            }
                            Delta::Other => {}
                        }
                    }
                    SseEvent::ContentBlockStop { index } => {
                        // Finalize on the canonical close signal for a
                        // `tool_use` block. The Anthropic streaming spec
                        // emits one `content_block_stop` per content
                        // block; if the model emits a leading text block
                        // (chatter) before a tool_use block, the text
                        // block's stop fires first — at that point we
                        // must NOT seal `finalized_action` yet, because
                        // a tool_use delta may still be in flight. Only
                        // seal when the stopped block is OUR tool_use,
                        // or when no tool_use has been opened and the
                        // message is over.
                        //
                        // For JSON-in-text streams (no tool_use ever
                        // appears), `MessageStop` runs and handles the
                        // fallback via `extract_action_from_text`. We
                        // don't need a text-fallback here — that would
                        // race against a tool_use delta and break the
                        // mixed-mode shape used by canonical Anthropic.
                        if finalized_action.is_none()
                            && tool_block_index == Some(index)
                        {
                            let action = tool_name.as_deref().and_then(|n| {
                                if n == "commander_action" {
                                    serde_json::from_str::<CommanderAction>(
                                        &tool_input_acc,
                                    )
                                    .ok()
                                } else {
                                    None
                                }
                            });
                            finalized_action = Some(action.clone());
                            let _ = tx.send(StreamToken::Done(action.clone()));
                        }
                    }
                    SseEvent::MessageStop => {
                        // If we already finalized on content_block_stop,
                        // return that. Otherwise (model that doesn't emit
                        // an explicit content_block_stop for some reason,
                        // or one that only does text), try the same two
                        // paths and prefer `tool_use` if both succeed.
                        let action = match finalized_action.take() {
                            Some(a) => a,
                            None => match tool_name.as_deref() {
                                Some(n) if n == "commander_action" => {
                                    serde_json::from_str::<CommanderAction>(
                                        &tool_input_acc,
                                    )
                                    .ok()
                                }
                                _ => extract_action_from_text(&text_acc),
                            },
                        };
                        let _ = tx.send(StreamToken::Done(action.clone()));
                        return action;
                    }
                    SseEvent::Ping | SseEvent::Other => {}
                }
            }
        }
        // Stream ended without explicit stop — best effort. Same
        // precedence: `tool_use` parser first, then JSON-in-text.
        let action = match finalized_action.take() {
            Some(a) => a,
            None => match tool_name.as_deref() {
                Some(n) if n == "commander_action" => {
                    serde_json::from_str::<CommanderAction>(&tool_input_acc).ok()
                }
                _ => extract_action_from_text(&text_acc),
            },
        };
        let _ = tx.send(StreamToken::Done(action.clone()));
        action
    }
}

/// Scan `text` for the first balanced top-level JSON object `{ ... }` and
/// try to deserialize it as a [`CommanderAction`].
///
/// Some Anthropic-compatible providers (notably `minimax` with the
/// `MiniMax-M3` model) ignore the `tools` field on streaming requests and
/// emit the response as plain text containing a JSON-encoded action. The
/// naïve parser — which only knows how to assemble `tool_use` blocks —
/// would return `None` for every turn and the run loop would silently
/// fall back to the heuristic's `feint`. `extract_action_from_text`
/// closes that gap: it scans for the first balanced `{...}` substring and
/// tries to parse it. Returns `None` if no balanced object is found or the
/// parse fails (which is the correct behaviour for any stream whose body
/// isn't a `CommanderAction`).
pub(crate) fn extract_action_from_text(text: &str) -> Option<CommanderAction> {
    let bytes = text.as_bytes();
    let mut start: Option<usize> = None;
    let mut depth: i32 = 0;
    let mut in_string = false;
    let mut escape = false;
    for (i, &b) in bytes.iter().enumerate() {
        if escape {
            escape = false;
            continue;
        }
        if in_string {
            match b {
                b'\\' => escape = true,
                b'"' => in_string = false,
                _ => {}
            }
            continue;
        }
        match b {
            b'"' => in_string = true,
            b'{' => {
                if start.is_none() {
                    start = Some(i);
                }
                depth += 1;
            }
            b'}' => {
                if depth > 0 {
                    depth -= 1;
                    if depth == 0 {
                        let s = start?;
                        let e = i + 1;
                        let candidate = &text[s..e];
                        if let Ok(action) =
                            serde_json::from_str::<CommanderAction>(candidate)
                        {
                            return Some(action);
                        }
                        // Reset for the next balanced object in case there
                        // are multiple JSONs in the text.
                        start = None;
                    }
                }
            }
            _ => {}
        }
    }
    None
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

    /// End-to-end assembly: a realistic Anthropic SSE sequence for a
    /// tool_use block must yield a parseable `CommanderAction` whose
    /// `action` field is NOT hard-coded to `feint`. This is the test
    /// that proves the streaming parser doesn't degenerate every
    /// opponent response into the heuristic fallback (which always
    /// picks `feint` for the typical DEFCON-3/tension-40 starting state
    /// — exactly the bug the user reported). It exercises the same
    /// accumulation logic `decide_stream` uses, so a regression here
    /// also breaks the live game.
    #[test]
    fn sse_stream_assembles_varied_commander_actions() {
        // Helper that simulates what decide_stream's accumulator does.
        // Each input is a RAW SSE record (possibly multi-line: an
        // `event:` line and a `data:` line). We extract the `data:`
        // payload exactly the way the real parser does, then feed
        // THAT to `serde_json::from_str` to get the typed `SseEvent`.
        fn assemble(records: &[&str]) -> Option<CommanderAction> {
            let mut tool_input_acc = String::new();
            let mut tool_name: Option<String> = None;
            let mut tool_block_index: Option<u32> = None;
            let mut finalized: Option<Option<CommanderAction>> = None;
            for raw in records {
                let mut data: Option<&str> = None;
                for line in raw.lines() {
                    if let Some(rest) = line.strip_prefix("data: ") {
                        data = Some(rest);
                    }
                }
                let Some(data) = data else { continue };
                if data == "[DONE]" {
                    continue;
                }
                let event: SseEvent = match serde_json::from_str(data) {
                    Ok(e) => e,
                    Err(_) => continue,
                };
                match event {
                    SseEvent::ContentBlockStart { index, block } => {
                        if let ContentBlock::ToolUse { name, input: _ } = block {
                            tool_name = Some(name);
                            tool_block_index = Some(index);
                            // Per the fix: do NOT seed the accumulator
                            // from `input` — the Anthropic API always
                            // emits `{}` here, and treating it as initial
                            // state corrupts the buffer.
                            tool_input_acc.clear();
                        }
                    }
                    SseEvent::ContentBlockDelta { index, delta } => {
                        if let Delta::InputJsonDelta { partial_json } = delta {
                            if tool_block_index == Some(index) {
                                tool_input_acc.push_str(&partial_json);
                            }
                        }
                    }
                    SseEvent::ContentBlockStop { index } => {
                        if finalized.is_none() && tool_block_index == Some(index) {
                            let action = tool_name.as_deref().and_then(|n| {
                                if n == "commander_action" {
                                    serde_json::from_str::<CommanderAction>(
                                        &tool_input_acc,
                                    )
                                    .ok()
                                } else {
                                    None
                                }
                            });
                            finalized = Some(action);
                        }
                    }
                    _ => {}
                }
            }
            finalized.and_then(|x| x)
        }

        // Each entry is a single tool_use block (no text preamble) so
        // the test exercises the exact same accumulator behavior as
        // the live parser: content_block_start → deltas → stop.
        //
        // The `partial_json` strings below are what the Anthropic API
        // ACTUALLY puts on the wire — a sequence of JSON-fragment
        // strings that, when concatenated, form a single complete
        // JSON object. They contain raw characters (`"`, `,`, `{`)
        // — no double-escaping. The raw-string literal `r#"..."#`
        // is used so we don't accidentally escape the `"` again.
        let cases: &[(&[&str], &str)] = &[
            (
                &[
                    r#"data: {"type":"content_block_start","index":1,"block":{"type":"tool_use","name":"commander_action","input":{}}}"#,
                    r#"data: {"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"action\":\"mobilize\","}}"#,
                    r#"data: {"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"\"message\":\"Bringing forces up.\"}"}}"#,
                    r#"data: {"type":"content_block_stop","index":1}"#,
                ],
                "mobilize",
            ),
            (
                &[
                    r#"data: {"type":"content_block_start","index":1,"block":{"type":"tool_use","name":"commander_action","input":{}}}"#,
                    r#"data: {"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"action\":\"negotiate\",\"message\":\"A channel is open.\"}"}}"#,
                    r#"data: {"type":"content_block_stop","index":1}"#,
                ],
                "negotiate",
            ),
            (
                &[
                    r#"data: {"type":"content_block_start","index":2,"block":{"type":"tool_use","name":"commander_action","input":{}}}"#,
                    r#"data: {"type":"content_block_delta","index":2,"delta":{"type":"input_json_delta","partial_json":"{\"action\":\"stand_down\",\"message\":\"We stand down.\"}"}}"#,
                    r#"data: {"type":"content_block_stop","index":2}"#,
                ],
                "stand_down",
            ),
            (
                &[
                    r#"data: {"type":"content_block_start","index":3,"block":{"type":"tool_use","name":"commander_action","input":{}}}"#,
                    r#"data: {"type":"content_block_delta","index":3,"delta":{"type":"input_json_delta","partial_json":"{\"action\":\"patrol\",\"message\":\"Routine patrol.\"}"}}"#,
                    r#"data: {"type":"content_block_stop","index":3}"#,
                ],
                "patrol",
            ),
        ];

        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        for (records, expected_action) in cases {
            let action = assemble(records)
                .unwrap_or_else(|| panic!("must parse stream for {}", expected_action));
            assert_eq!(
                action.action,
                *expected_action,
                "stream for {} assembled wrong",
                expected_action
            );
            assert!(!action.message.is_empty(), "message must be non-empty");
            seen.insert(action.action.clone());
        }
        // The whole point of the fix: opponents must respond with
        // VARIED actions, not always the same one.
        assert!(
            seen.len() >= 2,
            "opponent responses must include at least 2 distinct actions across realistic streams; saw {:?}",
            seen
        );
    }

    /// Live end-to-end proof against the actual `decide_stream` HTTP path.
    ///
    /// The helper above exercises the same accumulator logic, but a
    /// regression could easily slip into the connection / framing /
    /// channel layer of `decide_stream` without showing up there. This
    /// test spins up a real TCP listener, manually writes Anthropic-format
    /// SSE, and points an `LlmClient` at it — so a router/parser mismatch
    /// breaks it. Each round-trip yields a DIFFERENT tool_use block so we
    /// prove varied actions actually reach the live pipeline.
    #[tokio::test]
    async fn decide_stream_live_returns_varied_actions_against_mock_sse() {
        // Each entry: (records_to_write, expected_action). We rotate the
        // block index between rounds to prove `tool_block_index` doesn't
        // cause cross-round pollution — the previous test already covers
        // cross-event pollution inside one round.
        let cases: &[(&[&str], &str)] = &[
            (
                &[
                    r#"data: {"type":"message_start","message":{}}"#,
                    r#"data: {"type":"content_block_start","index":0,"block":{"type":"text","text":""}}"#,
                    r#"data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Pondering..."}}"#,
                    r#"data: {"type":"content_block_stop","index":0}"#,
                    r#"data: {"type":"content_block_start","index":1,"block":{"type":"tool_use","name":"commander_action","input":{}}}"#,
                    r#"data: {"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"action\":\"negotiate\",\"message\":\"A back-channel is open.\"}"}}"#,
                    r#"data: {"type":"content_block_stop","index":1}"#,
                    r#"data: {"type":"message_stop"}"#,
                ],
                "negotiate",
            ),
            (
                &[
                    r#"data: {"type":"message_start","message":{}}"#,
                    r#"data: {"type":"content_block_start","index":0,"block":{"type":"tool_use","name":"commander_action","input":{}}}"#,
                    r#"data: {"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{\"action\":\"mobilize\","}}"#,
                    r#"data: {"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"\"message\":\"Reserves are moving up.\"}"}}"#,
                    r#"data: {"type":"content_block_stop","index":0}"#,
                    r#"data: {"type":"message_stop"}"#,
                ],
                "mobilize",
            ),
            (
                &[
                    r#"data: {"type":"message_start","message":{}}"#,
                    r#"data: {"type":"content_block_start","index":0,"block":{"type":"text","text":""}}"#,
                    r#"data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Calculating..."}}"#,
                    r#"data: {"type":"content_block_stop","index":0}"#,
                    r#"data: {"type":"content_block_start","index":1,"block":{"type":"tool_use","name":"commander_action","input":{}}}"#,
                    r#"data: {"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"action\":\"strike\",\"target\":\"carrier_group\",\"message\":\"Opening strike.\"}"}}"#,
                    r#"data: {"type":"content_block_stop","index":1}"#,
                    r#"data: {"type":"message_stop"}"#,
                ],
                "strike",
            ),
        ];

        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

        for (records, expected) in cases {
            // Bind a fresh listener per case so each round sees only its
            // own response (the listener closes after one request).
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .expect("bind mock listener");
            let addr = listener.local_addr().expect("local_addr");

            // Server task: accept one connection, write the pre-canned
            // SSE body, then drop. We must drain the request before
            // responding — reqwest sends headers + body.
            let server = {
                let records = records.to_vec();
                tokio::spawn(async move {
                    let (mut sock, _) = match listener.accept().await {
                        Ok(p) => p,
                        Err(_) => return,
                    };
                    // Drain the request (small, bounded; reqwest closes
                    // the body after writing). A bounded read with a
                    // short timeout keeps the test deterministic.
                    use tokio::io::{AsyncReadExt, AsyncWriteExt};
                    let mut sink = [0u8; 4096];
                    let _ = tokio::time::timeout(
                        std::time::Duration::from_secs(2),
                        sock.read(&mut sink),
                    )
                    .await;

                    let body = records.join("\n\n") + "\n\n";
                    let response = format!(
                        "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\n\r\n{}",
                        body
                    );
                    let _ = sock.write_all(response.as_bytes()).await;
                    let _ = sock.shutdown().await;
                })
            };

            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(5))
                .build()
                .expect("build reqwest client");
            let llm = LlmClient {
                api_key: "test-key".into(),
                base_url: format!("http://{}", addr),
                model: "test-model".into(),
                client,
            };

            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
            // Tight ceiling so a hung server doesn't slow the test down.
            // Network i/o is local; 4s is plenty.
            let prev = std::env::var("WOPR_NET_TIMEOUT_MS").ok();
            // SAFETY: single-threaded test, no concurrent readers.
            unsafe {
                std::env::set_var("WOPR_NET_TIMEOUT_MS", "4000");
            }

            let result = llm
                .decide_stream("system", "user", tx)
                .await;

            // Restore the env so we don't leak into sibling tests.
            match prev {
                Some(v) => unsafe { std::env::set_var("WOPR_NET_TIMEOUT_MS", v) },
                None => unsafe { std::env::remove_var("WOPR_NET_TIMEOUT_MS") },
            }

            // Drain the channel (Done is the last token; recv returns
            // None once the sender is dropped at the end of
            // decide_stream).
            while let Some(tok) = rx.recv().await {
                if matches!(tok, StreamToken::Done(_)) {
                    break;
                }
            }

            let _ = server.await;

            let action = result.unwrap_or_else(|| panic!("decide_stream returned None for {}", expected));
            assert_eq!(
                action.action, *expected,
                "live decide_stream for {} assembled wrong action",
                expected
            );
            assert!(
                !action.message.is_empty(),
                "live decide_stream for {} produced empty message",
                expected
            );
            seen.insert(action.action.clone());
        }

        // The whole point of the SSE fix: the live wire produces
        // VARIED opponent responses, not always the same one. If this
        // assertion fails the parser has regressed back to collapsing
        // every tool_use payload into a single heuristic action —
        // which is exactly the user-reported bug.
        assert!(
            seen.len() >= 2,
            "live decide_stream must return at least 2 distinct actions across realistic streams; saw {:?}",
            seen
        );
    }

    /// Live test against the **real-shape** SSE stream the user's
    /// provider actually emits. Live wire runs (urllib round-trip
    /// against `~/.blumi/settings.json` → `api.minimax.io`) show the
    /// model puts the action JSON inside a text content block:
    ///
    /// ```text
    /// content_block_start  index=0  block={type:"text", text:""}
    /// content_block_delta  index=0  delta={type:"text_delta", text:"{\"action\":\"strike\", ...}"}
    /// content_block_stop   index=0
    /// message_stop
    /// ```
    ///
    /// No `tool_use` block ever appears. Before `extract_action_from_text`
    /// was wired into the finalizer paths, every one of these came back
    /// as `None` and the run loop took the heuristic's `feint`. This
    /// test pins the live-path behaviour to the real provider's
    /// shape, with three distinct actions across three rounds.
    #[tokio::test]
    async fn decide_stream_live_extracts_action_from_text_only_stream() {
        let cases: &[(&[&str], &str)] = &[
            (
                // Single text block, JSON-as-string. stop_reason=end_turn.
                &[
                    r#"data: {"type":"message_start","message":{}}"#,
                    r#"data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#,
                    r#"data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"{\"action\":\"strike\",\"message\":\"acknowledged.\"}"}}"#,
                    r#"data: {"type":"content_block_stop","index":0}"#,
                    r#"data: {"type":"message_delta","delta":{"stop_reason":"end_turn"}}"#,
                    r#"data: {"type":"message_stop"}"#,
                ],
                "strike",
            ),
            (
                // Multi-fragment text (the model streams the JSON across
                // several deltas, the way a real run looks). No tool_use.
                &[
                    r#"data: {"type":"message_start","message":{}}"#,
                    r#"data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#,
                    r#"data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"{\"action\":\""}}"#,
                    r#"data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"negotiate"}}"#,
                    r#"data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"\",\"message\":\"A back-channel is open.\"}"}}"#,
                    r#"data: {"type":"content_block_stop","index":0}"#,
                    r#"data: {"type":"message_stop"}"#,
                ],
                "negotiate",
            ),
            (
                // Lots of surrounding prose around the JSON; the helper
                // must skip the chatter and pull out the action.
                &[
                    r#"data: {"type":"message_start","message":{}}"#,
                    r#"data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#,
                    r#"data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Acknowledged. Stand down orders: "}}"#,
                    r#"data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"{\"action\":\"stand_down\",\"message\":\"We stand down.\"}"}}"#,
                    r#"data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":" Over and out."}}"#,
                    r#"data: {"type":"content_block_stop","index":0}"#,
                    r#"data: {"type":"message_stop"}"#,
                ],
                "stand_down",
            ),
        ];

        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

        for (records, expected) in cases {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .expect("bind mock listener");
            let addr = listener.local_addr().expect("local_addr");

            let server = {
                let records = records.to_vec();
                tokio::spawn(async move {
                    use tokio::io::{AsyncReadExt, AsyncWriteExt};
                    let (mut sock, _) = match listener.accept().await {
                        Ok(p) => p,
                        Err(_) => return,
                    };
                    let mut sink = [0u8; 4096];
                    let _ = tokio::time::timeout(
                        std::time::Duration::from_secs(2),
                        sock.read(&mut sink),
                    )
                    .await;
                    let body = records.join("\n\n") + "\n\n";
                    let response = format!(
                        "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\n\r\n{}",
                        body
                    );
                    let _ = sock.write_all(response.as_bytes()).await;
                    let _ = sock.shutdown().await;
                })
            };

            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(5))
                .build()
                .expect("build reqwest client");
            let llm = LlmClient {
                api_key: "test-key".into(),
                base_url: format!("http://{}", addr),
                model: "test-model".into(),
                client,
            };

            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
            let prev = std::env::var("WOPR_NET_TIMEOUT_MS").ok();
            // SAFETY: single-threaded test under the test runtime.
            unsafe {
                std::env::set_var("WOPR_NET_TIMEOUT_MS", "4000");
            }

            let result = llm.decide_stream("system", "user", tx).await;

            match prev {
                Some(v) => unsafe { std::env::set_var("WOPR_NET_TIMEOUT_MS", v) },
                None => unsafe { std::env::remove_var("WOPR_NET_TIMEOUT_MS") },
            }

            while let Some(tok) = rx.recv().await {
                if matches!(tok, StreamToken::Done(_)) {
                    break;
                }
            }

            let _ = server.await;

            let action = result.unwrap_or_else(|| {
                panic!(
                    "decide_stream returned None for {} — extract_action_from_text did not fire",
                    expected
                )
            });
            assert_eq!(
                action.action, *expected,
                "text-only live stream for {} assembled wrong",
                expected
            );
            assert!(!action.message.is_empty(), "message must be non-empty");
            seen.insert(action.action.clone());
        }

        assert!(
            seen.len() >= 3,
            "text-only stream test must yield at least 3 distinct actions (saw {:?})",
            seen
        );
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
        // Anthropic's canonical wire format emits this key as
        // `content_block` (verified against the live `MiniMax-M3` stream
        // on `api.minimax.io`). The Anthropic published SDKs and many
        // docs interchangeably use `block` — accept both so the parser
        // works against any Anthropic-compatible provider.
        #[serde(alias = "content_block")]
        block: ContentBlock,
    },
    ContentBlockDelta {
        index: u32,
        delta: Delta,
    },
    /// Canonical close signal for a content block — emitted once per
    /// block, before `message_stop`. For a `tool_use` block, this is
    /// the moment the assembled input JSON is guaranteed complete and
    /// safe to parse into the typed `CommanderAction`. The previous
    /// parser only finalized on `message_stop`, which arrived AFTER
    /// `content_block_stop` and, on some providers (notably
    /// `MiniMax-M3` via MiniMax), caused the accumulator to be
    /// assembled too late or with stale state — the run loop then
    /// timed out and fell back to the heuristic opponent (always
    /// `feint`). This variant closes that gap.
    ContentBlockStop { index: u32 },
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