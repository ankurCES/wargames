//! Event log.

use crate::language::Language;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LogEntry {
    pub turn: u32,
    pub side: String, // "us" | "opp" | "world"
    pub kind: String, // "action" | "trigger" | "outcome" | "prediction" | "comm"
    #[serde(default)]
    pub language: Language,
    pub message: String,
}

impl LogEntry {
    pub fn action(turn: u32, side: &str, message: impl Into<String>) -> Self {
        Self {
            turn,
            side: side.to_string(),
            kind: "action".to_string(),
            language: Language::default(),
            message: message.into(),
        }
    }

    pub fn trigger(turn: u32, message: impl Into<String>) -> Self {
        Self {
            turn,
            side: "world".to_string(),
            kind: "trigger".to_string(),
            language: Language::default(),
            message: message.into(),
        }
    }

    pub fn outcome(turn: u32, message: impl Into<String>) -> Self {
        Self {
            turn,
            side: "world".to_string(),
            kind: "outcome".to_string(),
            language: Language::default(),
            message: message.into(),
        }
    }

    /// A comm item — a side-channel message from one actor to another
    /// (e.g. a Soviet hotline transcript, a terror actor's broadcast,
    /// or a streaming LLM response mid-turn). Renders with its own
    /// color in `widget_log` so it visually separates from neutral
    /// outcomes and triggers.
    pub fn comm(turn: u32, side: &str, message: impl Into<String>) -> Self {
        Self::comm_with_lang(turn, side, Language::default(), message)
    }

    /// Like [`LogEntry::comm`] but tags the message with its source
    /// language so the TUI can render RTL scripts (Arabic, Hebrew)
    /// with the directional arrow on the visual leading edge.
    pub fn comm_with_lang(
        turn: u32,
        side: &str,
        language: Language,
        message: impl Into<String>,
    ) -> Self {
        Self {
            turn,
            side: side.to_string(),
            kind: "comm".to_string(),
            language,
            message: message.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserialize_without_language_defaults_to_english() {
        // Backward-compat: an old fixture with no `language` field
        // must still load — default is English.
        let json = r#"{"turn":1,"side":"opp","kind":"comm","message":"hi"}"#;
        let e: LogEntry = serde_json::from_str(json).expect("deserialize");
        assert_eq!(e.language, Language::English);
        assert_eq!(e.message, "hi");
    }

    #[test]
    fn deserialize_with_russian_language() {
        let json = r#"{"turn":2,"side":"opp","kind":"comm","language":"russian","message":"мы готовы"}"#;
        let e: LogEntry = serde_json::from_str(json).expect("deserialize");
        assert_eq!(e.language, Language::Russian);
    }

    #[test]
    fn comm_with_lang_builder_sets_fields() {
        let e = LogEntry::comm_with_lang(5, "opp", Language::Mandarin, "我们准备好了");
        assert_eq!(e.turn, 5);
        assert_eq!(e.side, "opp");
        assert_eq!(e.kind, "comm");
        assert_eq!(e.language, Language::Mandarin);
        assert_eq!(e.message, "我们准备好了");
    }

    #[test]
    fn comm_default_builder_is_english() {
        let e = LogEntry::comm(1, "opp", "hello");
        assert_eq!(e.language, Language::English);
    }
}