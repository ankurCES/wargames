//! World state model.

use crate::proxies::{Alliance, TerrorActor};
use serde::{Deserialize, Serialize};

/// Two superpowers in the original WOPR scenario. The country picker in the
/// TUI may extend this to other pairs (`NATO` vs `PRC`, etc.) but the core
/// engine currently models a bipolar standoff.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    Us,
    Opp,
}

impl Side {
    pub fn opposite(self) -> Self {
        match self {
            Side::Us => Side::Opp,
            Side::Opp => Side::Us,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Side::Us => "us",
            Side::Opp => "opp",
        }
    }
}

/// Strategic posture. Maps to escalation intent. The numeric ordering is
/// deliberately monotonic — higher = more aggressive — so rules can compare
/// without enumerating variants.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Posture {
    Negotiating,
    Deescalating,
    #[default]
    Routine,
    Aggressive,
    Hardened,
}

impl Posture {
    pub fn aggression_rank(self) -> u8 {
        match self {
            Posture::Negotiating => 0,
            Posture::Deescalating => 1,
            Posture::Routine => 2,
            Posture::Aggressive => 3,
            Posture::Hardened => 4,
        }
    }
}

/// Era affects trigger availability and the default scenario set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Era {
    ColdWar,
    Modern,
    NearPeer2030,
}

/// Theater — a region of operations. Each scenario names exactly one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Theater {
    BalticSea,
    BlackSea,
    KoreanPeninsula,
    TaiwanStrait,
    SouthChinaSea,
    RedSea,
    EasternMed,
    NorthAtlantic,
    Custom,
}

impl Theater {
    pub fn display_name(self) -> &'static str {
        match self {
            Theater::BalticSea => "Baltic Sea",
            Theater::BlackSea => "Black Sea",
            Theater::KoreanPeninsula => "Korean Peninsula",
            Theater::TaiwanStrait => "Taiwan Strait",
            Theater::SouthChinaSea => "South China Sea",
            Theater::RedSea => "Red Sea",
            Theater::EasternMed => "Eastern Mediterranean",
            Theater::NorthAtlantic => "North Atlantic",
            Theater::Custom => "Custom",
        }
    }
}

/// Which faction the player commands. The opponent is derived from the
/// scenario. This is what the country picker ultimately decides.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Faction {
    Us,
    Nato,
    Soviet,
    Prc,
    Dprk,
}

impl Faction {
    pub fn display_name(self) -> &'static str {
        match self {
            Faction::Us => "United States",
            Faction::Nato => "NATO (collective)",
            Faction::Soviet => "Soviet Union",
            Faction::Prc => "People's Republic of China",
            Faction::Dprk => "DPRK",
        }
    }
}

/// Per-side state — posture, escalation budget, ICBM count, etc.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct SideState {
    // `posture` defaults to `Posture::Routine` when missing so legacy
    // scenario JSON files (authored before `posture` was added) still
    // parse. Without this, every bundled `scenarios/*.json` fails
    // `serde_json::from_str` with `missing field 'posture'`, the
    // picker silently ends up with an empty scenario list, and the
    // user sees "the TUI has a problem after country is selected" —
    // the exact (a) bug. The struct-level `#[serde(default)]` makes
    // EVERY missing field use the `Default` impl below, so legacy
    // scenarios with completely-missing `SideState` blocks still load
    // and the player at least sees the scenario list.
    #[serde(default)]
    pub posture: Posture,
    #[serde(default)]
    pub escalation_budget: i32,
    #[serde(default)]
    pub silos_ready: u32,
    #[serde(default)]
    pub carriers_operational: u32,
    #[serde(default)]
    pub subs_at_sea: u32,
}

impl Default for SideState {
    fn default() -> Self {
        Self {
            posture: Posture::Routine,
            escalation_budget: 0,
            silos_ready: 0,
            carriers_operational: 0,
            subs_at_sea: 0,
        }
    }
}

impl SideState {
    pub fn default_player() -> Self {
        Self {
            posture: Posture::Routine,
            escalation_budget: 50,
            silos_ready: 100,
            carriers_operational: 3,
            subs_at_sea: 0,
        }
    }

    pub fn default_opponent() -> Self {
        Self {
            posture: Posture::Routine,
            escalation_budget: 50,
            silos_ready: 100,
            carriers_operational: 0,
            subs_at_sea: 6,
        }
    }
}

/// Full world state. One instance per game; cloned per turn.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorldState {
    pub turn: u32,
    pub era: Era,
    pub theater: Theater,
    pub faction: Faction,
    /// 0..=5 — 5 is peacetime, 1 is imminent strike.
    pub defcon: u8,
    /// 0..=100, derived from feeds + actions.
    pub tension: f32,
    /// 0..=100 — how much of opponent's posture the player can see.
    pub detection_pct: f32,
    pub sides: [SideState; 2],
    pub log: Vec<crate::log::LogEntry>,
    pub terminal: Option<crate::engine::GameOutcome>,
    /// Non-state actors (terror groups, insurgencies, PMCs) on the
    /// world stage. Defaults to empty so legacy scenarios without
    /// `terror_actors` still load and play.
    #[serde(default)]
    pub terror_actors: Vec<TerrorActor>,
    /// Bilateral alliances (treaties, proxy sponsorships, rivalries).
    /// Defaults to empty for backward-compat with legacy scenarios.
    #[serde(default)]
    pub alliances: Vec<Alliance>,
}

impl WorldState {
    pub fn side(&self, side: Side) -> &SideState {
        match side {
            Side::Us => &self.sides[0],
            Side::Opp => &self.sides[1],
        }
    }

    pub fn side_mut(&mut self, side: Side) -> &mut SideState {
        match side {
            Side::Us => &mut self.sides[0],
            Side::Opp => &mut self.sides[1],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn side_opposite_flips() {
        assert_eq!(Side::Us.opposite(), Side::Opp);
        assert_eq!(Side::Opp.opposite(), Side::Us);
    }

    #[test]
    fn posture_aggression_is_monotonic() {
        let order = [
            Posture::Negotiating,
            Posture::Deescalating,
            Posture::Routine,
            Posture::Aggressive,
            Posture::Hardened,
        ];
        for w in order.windows(2) {
            assert!(
                w[0].aggression_rank() < w[1].aggression_rank(),
                "{:?} must be less aggressive than {:?}",
                w[0],
                w[1]
            );
        }
    }
}