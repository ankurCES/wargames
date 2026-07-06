//! Action enum + per-action effects table.

use serde::{Deserialize, Serialize};

/// Strategic actions. Kept (8 from the JS impl) + new (3) for the predictive
/// event-driven rules + new (4) for the proxy / terror-actor layer.
///
/// Some actions carry a target id (e.g. the terror actor being funded or
/// the faction being sanctioned). For those, the engine reads the payload
/// from the surrounding context (scenario JSON, log entry metadata) — the
/// enum stays a flat tag so it can stay `Copy + Eq + Hash`.
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
    // --- new for the proxy / terror-actor layer (M5) ---
    /// Fund a proxy / terror actor. Raises their capability and
    /// loyalty (autonomy). Costs `escalation_budget`; raises tension.
    /// The actor id is read from the surrounding log entry metadata.
    FundProxy,
    /// Cut support to a proxy. Lowers their capability and raises
    /// their autonomy (they go freelance). Frees budget but raises
    /// tension because the actor retaliates independently.
    CutSupport,
    /// Strike a proxy / terror actor. Removes them from play if
    /// successful; raises opponent's detection of us; may drag
    /// their sponsor into the conflict if the proxy has low
    /// autonomy.
    StrikeProxy,
    /// Sanction a faction (state or non-state). Slows their
    /// mobilization; lower-impact than a strike but doesn't risk
    /// proxy chains.
    Sanction,
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
            Action::FundProxy => "fund_proxy",
            Action::CutSupport => "cut_support",
            Action::StrikeProxy => "strike_proxy",
            Action::Sanction => "sanction",
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
            Action::FundProxy => "FUND PROXY — bankroll a terror actor",
            Action::CutSupport => "CUT SUPPORT — abandon a proxy",
            Action::StrikeProxy => "STRIKE PROXY — eliminate a terror actor",
            Action::Sanction => "SANCTION — apply economic pressure",
        }
    }
}