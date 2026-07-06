//! Script / language tagging for comm entries.
//!
//! Used by `LogEntry` (and any future text-bearing field) to carry the
//! language of its content so the TUI can render the message with the
//! correct directional flow and bidi markers. The terminal itself only
//! needs to know whether the script is right-to-left — broad script
//! detection lives in [`Language::is_rtl`].
//!
//! Serde default is `English`, so existing JSON fixtures and serialized
//! game states without a `language` field keep loading unchanged.

use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Language {
    #[default]
    English,
    Russian,
    Mandarin,
    Korean,
    Arabic,
    Hebrew,
}

impl Language {
    /// Right-to-left scripts. Hebrew and Arabic.
    pub fn is_rtl(self) -> bool {
        matches!(self, Language::Arabic | Language::Hebrew)
    }

    /// Canonical tag (snake_case) — useful for filenames, scenario JSON,
    /// and the comm-entry serializer. Stable, never change the values
    /// without a migration.
    pub fn as_str(self) -> &'static str {
        match self {
            Language::English => "english",
            Language::Russian => "russian",
            Language::Mandarin => "mandarin",
            Language::Korean => "korean",
            Language::Arabic => "arabic",
            Language::Hebrew => "hebrew",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_english() {
        assert_eq!(Language::default(), Language::English);
    }

    #[test]
    fn rtl_classification() {
        assert!(Language::Arabic.is_rtl());
        assert!(Language::Hebrew.is_rtl());
        assert!(!Language::English.is_rtl());
        assert!(!Language::Russian.is_rtl());
        assert!(!Language::Mandarin.is_rtl());
        assert!(!Language::Korean.is_rtl());
    }

    #[test]
    fn tags_are_stable_snake_case() {
        // Tags must not change — they're persisted in scenario JSON.
        assert_eq!(Language::English.as_str(), "english");
        assert_eq!(Language::Russian.as_str(), "russian");
        assert_eq!(Language::Mandarin.as_str(), "mandarin");
        assert_eq!(Language::Korean.as_str(), "korean");
        assert_eq!(Language::Arabic.as_str(), "arabic");
        assert_eq!(Language::Hebrew.as_str(), "hebrew");
    }

    #[test]
    fn deserializes_missing_field_as_english() {
        // Backward-compat: a JSON fixture with no `language` field
        // must default to English so old saved games keep loading.
        #[derive(Deserialize)]
        struct Holder {
            #[serde(default)]
            language: Language,
        }
        let h: Holder = serde_json::from_str("{}").unwrap();
        assert_eq!(h.language, Language::English);
    }

    #[test]
    fn deserializes_explicit_russian() {
        #[derive(Deserialize)]
        struct Holder {
            language: Language,
        }
        let h: Holder = serde_json::from_str(r#"{"language":"russian"}"#).unwrap();
        assert_eq!(h.language, Language::Russian);
    }
}