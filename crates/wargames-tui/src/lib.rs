//! Wargames TUI — splash, country picker, herdr-style paned game UI.
//!
//! Public modules:
//! - [`config`]: `~/.blumi/settings.json` loader (hardcoded path).
//! - [`llm`]: anthropic-compatible LLM client.
//! - [`tts`]: optional ElevenLabs TTS (fails soft).
//! - [`net`]: 12-second shared ceiling.
//! - [`splash`]: 5-second "WAR GAMES OG" splash.
//! - [`picker`]: country + scenario picker.
//! - [`panes`]: herdr-style 2x2 + log layout.
//! - [`widget_state`], [`widget_predict`], [`widget_log`], [`widget_action`], [`widget_radar`].

pub mod app;
pub mod config;
pub mod llm;
pub mod net;
pub mod login;
pub mod panes;
pub mod picker;
pub mod settings;
pub mod splash;
pub mod text;
pub mod theme;
pub mod tts;
pub mod ui_anim;
pub mod widget_action;
pub mod widget_comms;
pub mod widget_defcon;
pub mod widget_log;
pub mod widget_predict;
pub mod widget_radar;
pub mod widget_receiving_popup;
pub mod widget_spinner;
pub mod widget_state;
pub mod widget_world;