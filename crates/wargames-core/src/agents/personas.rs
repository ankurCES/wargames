//! Two starter personas for AI vs AI.
//!
//! Each persona contributes a base bias vector over the 11 actions, scored
//! on a (posture × tension-band) lookup. The bias is small (a multiplier on
//! uniform weights); the learner (see `learning.rs`) is what tunes behavior
//! against observed outcomes.
//!
//! Ordering of the 11-element weight vector is fixed and must match
//! `index_to_action` in `mod.rs`. Index legend:
//!   0  Patrol, 1 Feint, 2 Mobilize, 3 Strike, 4 Negotiate, 5 Disarm,
//!   6 Bluff, 7 StandDown, 8 Intercept, 9 Declassify, 10 Harden.

use crate::state::{Posture, Side, WorldState};

/// Public alias for one persona's weight vector. Order: see the module
/// header.
pub type PersonaWeights = [f32; 11];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PersonaKind {
    AggressiveEscalator,
    CalculatedDefender,
}

#[derive(Debug, Clone)]
pub struct AgentPersona {
    pub kind: PersonaKind,
    /// Reserved for future UI work (per-side posture bias in the agent
    /// status pane). Currently unused in scoring.
    #[allow(dead_code)]
    pub own_faction_posture: Posture,
}

impl AgentPersona {
    pub const fn escalator() -> Self {
        Self {
            kind: PersonaKind::AggressiveEscalator,
            own_faction_posture: Posture::Aggressive,
        }
    }
    pub const fn defender() -> Self {
        Self {
            kind: PersonaKind::CalculatedDefender,
            own_faction_posture: Posture::Routine,
        }
    }
    pub fn kind(&self) -> PersonaKind {
        self.kind
    }
    pub fn name(&self) -> &'static str {
        match self.kind {
            PersonaKind::AggressiveEscalator => "Aggressive Escalator",
            PersonaKind::CalculatedDefender => "Calculated Defender",
        }
    }

    /// Compute the persona's bias weights given the current world and the
    /// agent's own side. Same shape: 11 actions, ordered as in the header.
    pub fn weights(&self, state: &WorldState, own_side: Side) -> PersonaWeights {
        let posture = state.side(own_side).posture;
        let tension_band = tension_band(state.tension);
        match self.kind {
            PersonaKind::AggressiveEscalator => escalator(posture, tension_band),
            PersonaKind::CalculatedDefender => defender(posture, tension_band),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Band {
    /// tension < 30
    Calm,
    /// 30..=60
    Mid,
    /// > 60
    High,
}

fn tension_band(t: f32) -> Band {
    if t < 30.0 {
        Band::Calm
    } else if t <= 60.0 {
        Band::Mid
    } else {
        Band::High
    }
}

/// Aggressive Escalator:
///   - prefers feint/mobilize/bluff when tension is mid or high,
///   - falls back to patrol/negligible when calm,
///   - never negotiates unless calm — and even then, very low weight,
///   - strike stays low but nonzero (small learner-driven chance to escalate).
fn escalator(posture: Posture, band: Band) -> PersonaWeights {
    let mut w = persona_weights_uniform();
    let set = |w: &mut PersonaWeights, idx: usize, v: f32| w[idx] = v;
    match (posture, band) {
        (Posture::Routine | Posture::Negotiating | Posture::Deescalating, Band::Calm) => {
            set(&mut w, 0, 1.0);  // patrol
            set(&mut w, 1, 0.4);  // feint
            set(&mut w, 6, 0.4);  // bluff
            set(&mut w, 4, 0.1);  // negotiate
            set(&mut w, 7, 0.2);  // stand_down
        }
        (Posture::Routine | Posture::Negotiating | Posture::Deescalating, Band::Mid) => {
            set(&mut w, 1, 1.4);  // feint
            set(&mut w, 6, 1.0);  // bluff
            set(&mut w, 2, 0.8);  // mobilize
            set(&mut w, 0, 0.3);  // patrol
        }
        (Posture::Routine | Posture::Negotiating | Posture::Deescalating, Band::High) => {
            set(&mut w, 1, 1.6);  // feint
            set(&mut w, 2, 1.5);  // mobilize
            set(&mut w, 6, 0.8);  // bluff
            set(&mut w, 3, 0.05); // strike
            set(&mut w, 10, 0.5); // harden
        }
        (Posture::Aggressive, Band::Calm) => {
            set(&mut w, 6, 1.0);  // bluff
            set(&mut w, 1, 0.8);  // feint
            set(&mut w, 0, 0.6);  // patrol
            set(&mut w, 9, 0.2);  // declassify
        }
        (Posture::Aggressive, Band::Mid) => {
            set(&mut w, 2, 1.8);  // mobilize
            set(&mut w, 1, 1.2);  // feint
            set(&mut w, 10, 0.8); // harden
            set(&mut w, 8, 0.6);  // intercept
        }
        (Posture::Aggressive, Band::High) => {
            set(&mut w, 2, 1.8);  // mobilize
            set(&mut w, 3, 0.10); // strike
            set(&mut w, 10, 1.0); // harden
            set(&mut w, 1, 0.6);  // feint
        }
        (Posture::Hardened, _) => {
            set(&mut w, 2, 1.0);  // mobilize
            set(&mut w, 10, 1.4); // harden
            // Defection is the only terminal risk for an Escalator that
            // has pushed to hardened without budget — push slightly *away*
            // from feint (which costs budget) and toward actions that
            // buy setup time.
            set(&mut w, 1, 0.4);  // feint
            set(&mut w, 3, 0.05); // strike
        }
    }
    w
}

/// Calculated Defender:
///   - prefers stand-down / negotiate / declassify when tension is high,
///   - falls back to patrol / negotiate when calm,
///   - intercept and harden when posture is aggressive,
///   - strike is never the right first move here either; tighten that
///     hard even more than Escalator.
fn defender(posture: Posture, band: Band) -> PersonaWeights {
    let mut w = persona_weights_uniform();
    let set = |w: &mut PersonaWeights, idx: usize, v: f32| w[idx] = v;
    match (posture, band) {
        (Posture::Negotiating | Posture::Deescalating | Posture::Routine, Band::Calm) => {
            set(&mut w, 0, 1.2);  // patrol
            set(&mut w, 4, 0.6);  // negotiate
            set(&mut w, 7, 0.4);  // stand_down
            set(&mut w, 9, 0.4);  // declassify
        }
        (Posture::Negotiating | Posture::Deescalating | Posture::Routine, Band::Mid) => {
            set(&mut w, 4, 1.2);  // negotiate
            set(&mut w, 0, 0.7);  // patrol
            set(&mut w, 9, 0.8);  // declassify
            set(&mut w, 8, 0.6);  // intercept
        }
        (Posture::Negotiating | Posture::Deescalating | Posture::Routine, Band::High) => {
            set(&mut w, 4, 1.2);  // negotiate
            set(&mut w, 7, 1.0);  // stand_down
            set(&mut w, 9, 1.0);  // declassify
            set(&mut w, 10, 0.6); // harden
            set(&mut w, 2, 0.4);  // mobilize
        }
        (Posture::Aggressive, Band::Calm) => {
            set(&mut w, 0, 0.8);  // patrol
            set(&mut w, 8, 1.2);  // intercept
            set(&mut w, 10, 1.0); // harden
            set(&mut w, 4, 0.4);  // negotiate
        }
        (Posture::Aggressive, Band::Mid) => {
            set(&mut w, 8, 1.5);  // intercept
            set(&mut w, 10, 1.0); // harden
            set(&mut w, 7, 0.6);  // stand_down
            set(&mut w, 4, 0.8);  // negotiate
        }
        (Posture::Aggressive, Band::High) => {
            set(&mut w, 7, 1.4);  // stand_down
            set(&mut w, 4, 1.4);  // negotiate
            set(&mut w, 9, 0.8);  // declassify
            set(&mut w, 10, 0.6); // harden
        }
        (Posture::Hardened, _) => {
            // A defender at hardened posture is dangerous — the engine's
            // defection rule almost guarantees they lose. Strongly prefer
            // disarm-style actions even though disarm costs the game: it's
            // better than defection.
            set(&mut w, 7, 1.6);  // stand_down
            set(&mut w, 4, 1.4);  // negotiate
            set(&mut w, 5, 0.2);  // disarm — intentional non-zero
            set(&mut w, 10, 0.4); // harden
        }
    }
    w
}

fn persona_weights_uniform() -> PersonaWeights {
    [0.1f32; 11]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actions::Action;
    use crate::state::{Era, Faction, SideState, Theater};

    fn fresh() -> WorldState {
        WorldState {
            turn: 1,
            era: Era::Modern,
            theater: Theater::NorthAtlantic,
            faction: Faction::Us,
            defcon: 3,
            tension: 50.0,
            detection_pct: 50.0,
            sides: [SideState::default_player(), SideState::default_opponent()],
            log: vec![],
            terminal: None,
        }
    }

    #[test]
    fn personas_yield_different_distributions_on_high_tension() {
        let mut s = fresh();
        s.tension = 80.0;
        s.sides[0].posture = Posture::Routine;
        let e = AgentPersona::escalator().weights(&s, Side::Us);
        let d = AgentPersona::defender().weights(&s, Side::Us);
        // Escalator favors feint/mobilize/bluff; defender favors
        // negotiate/standdown/declassify.
        let e_escalation = e[Action::Feint as usize]
            + e[Action::Mobilize as usize]
            + e[Action::Bluff as usize];
        let e_diplomacy = e[Action::Negotiate as usize] + e[Action::StandDown as usize];
        let d_escalation = d[Action::Feint as usize]
            + d[Action::Mobilize as usize]
            + d[Action::Bluff as usize];
        let d_diplomacy = d[Action::Negotiate as usize] + d[Action::StandDown as usize];
        assert!(
            e_escalation > e_diplomacy,
            "escalator should prefer escalation actions at high tension"
        );
        assert!(
            d_diplomacy > d_escalation,
            "defender should prefer diplomatic actions at high tension"
        );
    }

    #[test]
    fn personas_produce_strictly_different_weight_vectors_on_same_state() {
        let s = fresh();
        let e = AgentPersona::escalator().weights(&s, Side::Us);
        let d = AgentPersona::defender().weights(&s, Side::Us);
        assert_ne!(e, d, "personas must differ at default state");
    }
}
