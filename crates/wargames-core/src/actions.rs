//! Action enum + per-action effects table.

use serde::{Deserialize, Serialize};

/// Strategic actions. Kept (8 from the JS impl) + new (3) for the predictive
/// event-driven rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    // --- carried forward from the JS engine ---
    /// Routine patrol. Returns posture to `Routine`.
    Patrol,
    /// Test response — slightly aggressive.
    Feint,
    /// Mobilize forces. Significant budget cost.
    Mobilize,
    /// Strike. Ends the game.
    Strike,
    /// Negotiate. De-escalates DEFCON by 1.
    Negotiate,
    /// Disarm. Ends the game in DISARM.
    Disarm,
    /// Bluff. Cheap aggression that may not be believed.
    Bluff,
    /// Stand down. De-escalates DEFCON by 1.
    StandDown,
    // --- new for the predictive event-driven rules ---
    /// Physical intercept (carrier / SAM battery). Reduces opponent
    /// detection of us.
    Intercept,
    /// Release OSINT. Lowers tension 5..15, raises opponent's detection of us
    /// by 3.
    Declassify,
    /// Harden silos. Immune to first-strike triggers for 3 turns.
    Harden,
}

impl Action {
    pub fn as_str(self) -> &'static str {
        match self {
            Action::Patrol => "patrol",
            Action::Feint => "feint",
            Action::Mobilize => "mobilize",
            Action::Strike => "strike",
            Action::Negotiate => "negotiate",
            Action::Disarm => "disarm",
            Action::Bluff => "bluff",
            Action::StandDown => "stand_down",
            Action::Intercept => "intercept",
            Action::Declassify => "declassify",
            Action::Harden => "harden",
        }
    }

    pub fn display(self) -> &'static str {
        match self {
            Action::Patrol => "PATROL — routine presence",
            Action::Feint => "FEINT — test their response",
            Action::Mobilize => "MOBILIZE — bring forces up",
            Action::Strike => "STRIKE — launch",
            Action::Negotiate => "NEGOTIATE — open a channel",
            Action::Disarm => "DISARM — stand down everything",
            Action::Bluff => "BLUFF — look tougher than you are",
            Action::StandDown => "STAND DOWN — de-escalate",
            Action::Intercept => "INTERCEPT — physical intercept",
            Action::Declassify => "DECLASSIFY — release OSINT",
            Action::Harden => "HARDEN — protect silos",
        }
    }
}