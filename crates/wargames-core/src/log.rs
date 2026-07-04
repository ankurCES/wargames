//! Event log.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LogEntry {
    pub turn: u32,
    pub side: String, // "us" | "opp" | "world"
    pub kind: String, // "action" | "trigger" | "outcome" | "prediction"
    pub message: String,
}

impl LogEntry {
    pub fn action(turn: u32, side: &str, message: impl Into<String>) -> Self {
        Self {
            turn,
            side: side.to_string(),
            kind: "action".to_string(),
            message: message.into(),
        }
    }

    pub fn trigger(turn: u32, message: impl Into<String>) -> Self {
        Self {
            turn,
            side: "world".to_string(),
            kind: "trigger".to_string(),
            message: message.into(),
        }
    }

    pub fn outcome(turn: u32, message: impl Into<String>) -> Self {
        Self {
            turn,
            side: "world".to_string(),
            kind: "outcome".to_string(),
            message: message.into(),
        }
    }
}