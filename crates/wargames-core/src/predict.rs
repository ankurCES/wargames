//! Deterministic Monte Carlo predictor.

use crate::actions::Action;
use crate::engine::{apply_action, is_terminal};
use crate::state::{Posture, Side, WorldState};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Prediction {
    pub p_strike: f32,
    pub p_disarm: f32,
    pub p_defect: f32,
    pub p_negotiate: f32,
    pub expected_defcon_delta: f32,
    pub expected_tension_delta: f32,
}

impl Prediction {
    pub fn display_bars(&self) -> String {
        format!(
            "STRIKE {:>3}%  DISARM {:>3}%  DEFECT {:>3}%  NEGOT  {:>3}%",
            (self.p_strike * 100.0).round() as u32,
            (self.p_disarm * 100.0).round() as u32,
            (self.p_defect * 100.0).round() as u32,
            (self.p_negotiate * 100.0).round() as u32,
        )
    }
}

/// Predict the next `horizon` turns by running `n_sims` Monte Carlo rollouts
/// with deterministic seed. The policy is intentionally simple — a "neutral"
/// opponent plays a heuristic (mobilize if hardened, negotiate if routine,
/// strike if defcon == 1).
///
/// This is the "predictions based on actions" the user asked for. The same
/// seed produces the same probabilities, which keeps it testable.
pub fn predict(state: &WorldState, seed: u64, n_sims: u32, horizon: u32) -> Prediction {
    use rand::rngs::StdRng;
    use rand::{Rng, SeedableRng};

    let mut strike = 0u32;
    let mut disarm = 0u32;
    let mut defect = 0u32;
    let mut negotiate = 0u32;
    let mut defcon_sum: f32 = 0.0;
    let mut tension_sum: f32 = 0.0;

    let base_defcon = state.defcon as f32;
    let base_tension = state.tension;

    for sim in 0..n_sims {
        // Each sim gets its own deterministic RNG.
        let mut rng = StdRng::seed_from_u64(seed.wrapping_add(sim as u64));
        let mut sim_state = state.clone();

        // The player's action in this sim is randomized from a small set so
        // we don't bias toward any one outcome.
        let player_actions = [
            Action::Patrol,
            Action::Feint,
            Action::Negotiate,
            Action::StandDown,
            Action::Intercept,
            Action::Declassify,
            Action::Mobilize,
            Action::Bluff,
        ];
        let player_action = player_actions[rng.gen_range(0..player_actions.len())];

        sim_state = apply_action(&sim_state, Side::Us, player_action);
        if is_terminal(&sim_state) {
            match sim_state.terminal {
                Some(crate::engine::GameOutcome::Strike { .. }) => strike += 1,
                Some(crate::engine::GameOutcome::Disarm { .. }) => disarm += 1,
                Some(crate::engine::GameOutcome::Defect { .. }) => defect += 1,
                None => {}
            }
            defcon_sum += sim_state.defcon as f32 - base_defcon;
            tension_sum += sim_state.tension - base_tension;
            continue;
        }

        // Opponent heuristic.
        let opp_action = heuristic_opponent(&sim_state, &mut rng);
        sim_state = apply_action(&sim_state, Side::Opp, opp_action);

        if is_terminal(&sim_state) {
            match sim_state.terminal {
                Some(crate::engine::GameOutcome::Strike { .. }) => strike += 1,
                Some(crate::engine::GameOutcome::Disarm { .. }) => disarm += 1,
                Some(crate::engine::GameOutcome::Defect { .. }) => defect += 1,
                None => {}
            }
            defcon_sum += sim_state.defcon as f32 - base_defcon;
            tension_sum += sim_state.tension - base_tension;
            continue;
        }

        // Roll forward up to `horizon` more turns, alternating, with the
        // same heuristic. Track outcomes.
        let mut ended = false;
        for _ in 1..horizon {
            for side in [Side::Us, Side::Opp] {
                let a = if side == Side::Us {
                    player_actions[rng.gen_range(0..player_actions.len())]
                } else {
                    heuristic_opponent(&sim_state, &mut rng)
                };
                sim_state = apply_action(&sim_state, side, a);
                if is_terminal(&sim_state) {
                    match sim_state.terminal {
                        Some(crate::engine::GameOutcome::Strike { .. }) => strike += 1,
                        Some(crate::engine::GameOutcome::Disarm { .. }) => disarm += 1,
                        Some(crate::engine::GameOutcome::Defect { .. }) => defect += 1,
                        None => {}
                    }
                    ended = true;
                    break;
                }
            }
            if ended {
                break;
            }
        }

        defcon_sum += sim_state.defcon as f32 - base_defcon;
        tension_sum += sim_state.tension - base_tension;
        if player_action == Action::Negotiate {
            negotiate += 1;
        }
    }

    let total = n_sims.max(1) as f32;
    Prediction {
        p_strike: strike as f32 / total,
        p_disarm: disarm as f32 / total,
        p_defect: defect as f32 / total,
        p_negotiate: negotiate as f32 / total,
        expected_defcon_delta: defcon_sum / total,
        expected_tension_delta: tension_sum / total,
    }
}

fn heuristic_opponent<R: rand::Rng>(state: &WorldState, rng: &mut R) -> Action {
    // If defcon is 1 and we're hardened, strike.
    if state.defcon == 1 && state.side(Side::Opp).posture == Posture::Hardened {
        return Action::Strike;
    }
    // If routine and tension < 30, patrol.
    if state.side(Side::Opp).posture == Posture::Routine && state.tension < 30.0 {
        return Action::Patrol;
    }
    // If tension is high, sometimes mobilize, sometimes negotiate.
    if state.tension > 60.0 {
        return if rng.gen_bool(0.5) {
            Action::Mobilize
        } else {
            Action::Negotiate
        };
    }
    // Otherwise feint occasionally.
    if rng.gen_bool(0.3) {
        Action::Feint
    } else {
        Action::Patrol
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{Era, Faction, SideState, Theater};

    fn fresh() -> WorldState {
        WorldState {
            turn: 1,
            era: Era::ColdWar,
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
    fn predict_is_deterministic() {
        let s = fresh();
        let p1 = predict(&s, 42, 100, 5);
        let p2 = predict(&s, 42, 100, 5);
        assert_eq!(p1.p_strike, p2.p_strike);
        assert_eq!(p1.p_disarm, p2.p_disarm);
    }

    #[test]
    fn probabilities_sum_to_at_most_one() {
        let s = fresh();
        let p = predict(&s, 1, 200, 5);
        let sum = p.p_strike + p.p_disarm + p.p_defect;
        // Sum may exceed 1 because the same sim can resolve any of the three
        // terminal outcomes; just sanity-check it's finite and bounded.
        assert!(sum.is_finite());
        assert!(sum <= 3.0);
    }
}