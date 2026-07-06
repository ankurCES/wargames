//! Wargames core engine — pure rules, no I/O.
//!
//! Modules:
//! - [`state`]: `WorldState`, `Side`, `Posture`, `Era`, `Theater`.
//! - [`actions`]: action matrix with effects table.
//! - [`engine`]: turn application + terminal detection.
//! - [`triggers`]: world-event triggers that fire when conditions are met.
//! - [`predict`]: deterministic Monte Carlo predictor.
//! - [`scenario`]: `serde_json` loader for `scenarios/*.json`.
//! - [`log`]: `LogEntry` + helpers.

pub mod actions;
pub mod agents;
pub mod engine;
pub mod language;
pub mod log;
pub mod predict;
pub mod proxies;
pub mod scenario;
pub mod state;
pub mod triggers;

pub use state::{Era, Faction, Posture, Side, SideState, Theater, WorldState};
pub use actions::Action;
pub use engine::{apply_action, game_over, is_terminal, GameOutcome};
pub use language::Language;
pub use log::LogEntry;
pub use predict::{predict, Prediction};
pub use proxies::{Alliance, AllianceKind, Sponsor, TerrorActor};
pub use scenario::{Scenario, WinConditions};
pub use triggers::{Trigger, TriggerId};