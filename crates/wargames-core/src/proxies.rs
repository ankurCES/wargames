//! Non-state actors and the alliance / sponsorship web that connects
//! them to the two great-power sides.
//!
//! The original WOPR scenario treats the world as bipolar — US vs.
//! USSR — but real escalation is shaped by **proxies** (terror
//! actors, insurgencies, mercenary groups) whose behavior is partially
//! steered by their sponsor and partially autonomous. This module
//! adds:
//!
//!   - [`TerrorActor`] — a non-state group with its own capability,
//!     radicalization, autonomy, and a [`Sponsor`] link back to US,
//!     Opp, or independent.
//!   - [`Alliance`] — the relationship between two actors (or an
//!     actor and a faction): treaty, proxy sponsorship, or rivalry.
//!
//! The engine uses these to add a fourth DEFCON-1 trigger: a proxy
//! strike that drags its sponsor into the conflict. See
//! `triggers::proxy_strike_fires`.

use serde::{Deserialize, Serialize};

/// Who sponsors a terror actor — i.e. who funds / arms / trains them.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sponsor {
    Us,
    #[default]
    Opp,
    /// No great-power sponsor — e.g. a homegrown insurgency, an
    /// ideological group, or a PMC. These actors follow their own
    /// agenda and cannot be reliably deterred by signalling to a
    /// sponsor.
    Independent,
}

impl Sponsor {
    pub fn as_str(self) -> &'static str {
        match self {
            Sponsor::Us => "us",
            Sponsor::Opp => "opp",
            Sponsor::Independent => "independent",
        }
    }
}

/// A non-state actor (terror group, insurgency, PMC, etc.) on the
/// world stage. Each one is small but can detonate an escalation
/// path if its radicalization crosses a threshold or if it conducts
/// a strike on the opposing side.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TerrorActor {
    /// Stable id (e.g. `"crescent-falcons"`). Used as the action
    /// target and in scenario JSON.
    pub id: String,
    /// Display name for the log and picker.
    pub name: String,
    /// Region of operations — a free-form label for now; we don't
    /// model geography finely enough for it to gate actions.
    pub region: String,
    /// Who backs them. `Independent` actors cannot be deterred
    /// through their sponsor.
    pub sponsor: Sponsor,
    /// 0..=100 — operational capacity (people, weapons, territory).
    #[serde(default = "default_capability")]
    pub capability: u8,
    /// 0..=100 — willingness to use violence. At ≥ 80 a `StrikeProxy`
    /// trigger becomes possible even without sponsor approval.
    #[serde(default = "default_radicalization")]
    pub radicalization: u8,
    /// 0..=100 — how much they follow sponsor direction. Low
    /// autonomy = highly dependent; high autonomy = freelancing.
    /// Defaults to 50 — a moderate proxy that mostly follows orders
    /// but occasionally acts on its own.
    #[serde(default = "default_autonomy")]
    pub autonomy: u8,
}

fn default_capability() -> u8 {
    30
}
fn default_radicalization() -> u8 {
    40
}
fn default_autonomy() -> u8 {
    50
}

impl TerrorActor {
    /// Probability (0..=100) that this actor freelances a strike on
    /// their own initiative this turn, without sponsor approval.
    /// Roughly `radicalization * autonomy / 100`. Capped at 99 to
    /// leave room for sponsor-controlled actions.
    pub fn freelance_strike_risk(&self) -> u8 {
        ((u32::from(self.radicalization) * u32::from(self.autonomy)) / 100)
            .min(99) as u8
    }
}

/// The kind of relationship between two sides of an alliance. Used
/// by the engine to gate which actions are available and how they
/// propagate effects.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AllianceKind {
    /// Mutual defense — an attack on either partner drags the other
    /// in (NATO Article 5). Funded by `escalation_budget`.
    #[default]
    Treaty,
    /// Sponsor → proxy — sponsor can `FundProxy` / `CutSupport` and
    /// proxy strikes risk dragging sponsor in if autonomy is low.
    Proxy,
    /// Active rivalry — actions against one are seen as actions
    /// against the other (no automatic treaty obligations, but
    /// tension multipliers).
    Rivalry,
}

impl AllianceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            AllianceKind::Treaty => "treaty",
            AllianceKind::Proxy => "proxy",
            AllianceKind::Rivalry => "rivalry",
        }
    }
}

/// A bilateral alliance. `sides` is `[Side, Side]` ordered so that
/// `sides[0]` is the "primary" side (e.g. the sponsor for a proxy
/// alliance, the larger party for a treaty).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Alliance {
    pub sides: [Side; 2],
    pub kind: AllianceKind,
    /// 0..=100 — strength of the bond. Treaty alliances > 80 typically
    /// trigger automatic mutual defense; < 30 means the bond is
    /// mostly performative and may collapse on crisis.
    #[serde(default = "default_strength")]
    pub strength: u8,
}

fn default_strength() -> u8 {
    70
}

impl Alliance {
    pub fn involves(&self, side: Side) -> bool {
        self.sides[0] == side || self.sides[1] == side
    }
}

/// `use crate::state::Side;` — re-exported here for the alliance
/// type. Kept in this module to avoid adding a new public
/// dependency from `state.rs`.
pub(crate) use crate::state::Side;

#[cfg(test)]
mod tests {
    use super::*;

    fn actor(sponsor: Sponsor, rad: u8, aut: u8) -> TerrorActor {
        TerrorActor {
            id: "test".into(),
            name: "Test".into(),
            region: "test".into(),
            sponsor,
            capability: 30,
            radicalization: rad,
            autonomy: aut,
        }
    }

    #[test]
    fn freelance_strike_risk_is_radicalization_times_autonomy() {
        // 80% radicalization, 50% autonomy → 40
        let a = actor(Sponsor::Opp, 80, 50);
        assert_eq!(a.freelance_strike_risk(), 40);
        // 100% × 100% → capped at 99
        let a = actor(Sponsor::Us, 100, 100);
        assert_eq!(a.freelance_strike_risk(), 99);
        // 0% × anything → 0
        let a = actor(Sponsor::Independent, 0, 80);
        assert_eq!(a.freelance_strike_risk(), 0);
    }

    #[test]
    fn sponsor_serializes_as_snake_case() {
        let json = serde_json::to_string(&Sponsor::Independent).unwrap();
        assert_eq!(json, "\"independent\"");
        let json = serde_json::to_string(&Sponsor::Us).unwrap();
        assert_eq!(json, "\"us\"");
    }

    #[test]
    fn actor_defaults_for_missing_capability_and_autonomy() {
        // Backward-compat: old scenario JSON without `capability` /
        // `radicalization` / `autonomy` must still parse, using
        // sensible defaults (30 / 40 / 50).
        let json = r#"{
            "id": "x",
            "name": "X",
            "region": "y",
            "sponsor": "us"
        }"#;
        let a: TerrorActor = serde_json::from_str(json).unwrap();
        assert_eq!(a.capability, 30);
        assert_eq!(a.radicalization, 40);
        assert_eq!(a.autonomy, 50);
    }

    #[test]
    fn alliance_involves_checks_both_sides() {
        use crate::state::Side;
        let a = Alliance {
            sides: [Side::Us, Side::Opp],
            kind: AllianceKind::Treaty,
            strength: 80,
        };
        assert!(a.involves(Side::Us));
        assert!(a.involves(Side::Opp));
    }

    #[test]
    fn alliance_kind_serializes_as_snake_case() {
        let json = serde_json::to_string(&AllianceKind::Proxy).unwrap();
        assert_eq!(json, "\"proxy\"");
    }
}