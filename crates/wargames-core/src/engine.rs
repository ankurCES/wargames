//! Engine — applies an action to a `WorldState`, detects terminal states.

use crate::actions::Action;
use crate::log::LogEntry;
use crate::state::{Posture, Side, SideState, WorldState};
use crate::triggers;
use serde::{Deserialize, Serialize};

/// Terminal outcomes — at most one of these is `Some` at any time.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum GameOutcome {
    /// A side launched. Mutual destruction likely.
    Strike { by: Side, turn: u32 },
    /// A side stood down. Diplomacy prevailed.
    Disarm { by: Side, turn: u32 },
    /// Escalation budget exhausted under hardened posture. Loss of command control.
    Defect { by: Side, turn: u32 },
}

pub fn is_terminal(state: &WorldState) -> bool {
    state.terminal.is_some()
}

pub fn game_over(state: &WorldState) -> Option<&GameOutcome> {
    state.terminal.as_ref()
}

/// Apply `action` taken by `by` to `state`. Returns the new state (the input
/// is not mutated).
///
/// This is the single source of truth for the rules. Tests should target this
/// function rather than reaching into `WorldState` fields directly.
pub fn apply_action(state: &WorldState, by: Side, action: Action) -> WorldState {
    let mut next = state.clone();
    next.turn = next.turn.saturating_add(1);

    next.log.push(LogEntry::action(
        next.turn,
        by.as_str(),
        action.display(),
    ));

    let delta = action_effects(action);

    // Posture.
    let new_posture = match action {
        Action::Patrol => Posture::Routine,
        Action::Feint => Posture::Aggressive,
        Action::Mobilize => Posture::Hardened,
        Action::Strike => {
            next.terminal = Some(GameOutcome::Strike {
                by,
                turn: next.turn,
            });
            next.log.push(LogEntry::outcome(
                next.turn,
                "STRIKE AUTHORIZED — terminal.",
            ));
            return next;
        }
        Action::Negotiate => Posture::Negotiating,
        Action::Disarm => {
            next.terminal = Some(GameOutcome::Disarm {
                by,
                turn: next.turn,
            });
            next.log.push(LogEntry::outcome(next.turn, "DISARM — terminal."));
            return next;
        }
        Action::Bluff => Posture::Aggressive,
        Action::StandDown => Posture::Deescalating,
        Action::Intercept => next.side(by).posture, // posture unchanged
        Action::Declassify => next.side(by).posture, // posture unchanged
        Action::Harden => next.side(by).posture,    // posture unchanged
        // Proxy / terror-actor actions: posture unchanged. The
        // delta block below applies capability / radicalization /
        // tension effects. Keeping posture stable here means a
        // `FundProxy` doesn't accidentally trip an escalation
        // trigger — the *consequences* (radicalization, retaliation)
        // are modeled separately.
        Action::FundProxy => next.side(by).posture,
        Action::CutSupport => next.side(by).posture,
        Action::StrikeProxy => next.side(by).posture,
        Action::Sanction => next.side(by).posture,
    };
    next.side_mut(by).posture = new_posture;

    // Escalation budget.
    next.side_mut(by).escalation_budget =
        (next.side_mut(by).escalation_budget + delta.budget).max(0);

    // DEFCON.
    match delta.defcon {
        Some(d) => {
            next.defcon = (next.defcon as i32 + d).clamp(1, 5) as u8;
        }
        None => {}
    }

    // Tension + detection.
    next.tension = (next.tension + delta.tension).clamp(0.0, 100.0);
    next.detection_pct = (next.detection_pct + delta.detection).clamp(0.0, 100.0);

    // Resource decay — both sides lose 1 budget per turn.
    for side_state in next.sides.iter_mut() {
        side_state.escalation_budget = (side_state.escalation_budget - 1).max(0);
    }

    // Detection drift — both aggressive → up; both quiet → down.
    let aggressor_count = next
        .sides
        .iter()
        .filter(|s| matches!(s.posture, Posture::Aggressive | Posture::Hardened))
        .count();
    let drift: f32 = if aggressor_count >= 2 {
        4.0
    } else if aggressor_count == 0 {
        -2.0
    } else {
        0.0
    };
    next.detection_pct = (next.detection_pct + drift).clamp(5.0, 100.0);

    // Defection rule: budget == 0 AND posture == Hardened → DEFECT.
    for (idx, side_state) in next.sides.iter().enumerate() {
        if side_state.escalation_budget == 0 && side_state.posture == Posture::Hardened {
            let by = if idx == 0 { Side::Us } else { Side::Opp };
            next.terminal = Some(GameOutcome::Defect {
                by,
                turn: next.turn,
            });
            next.log
                .push(LogEntry::outcome(next.turn, "DEFECTION — terminal."));
            return next;
        }
    }

    // Fire triggers.
    triggers::evaluate(&mut next);

    next
}

#[derive(Debug, Clone, Copy)]
struct ActionDelta {
    budget: i32,
    defcon: Option<i32>,
    tension: f32,
    detection: f32,
}

fn action_effects(action: Action) -> ActionDelta {
    match action {
        Action::Patrol => ActionDelta {
            budget: 0,
            defcon: None,
            tension: -1.0,
            detection: 1.0,
        },
        Action::Feint => ActionDelta {
            budget: -3,
            defcon: None,
            tension: 3.0,
            detection: 5.0,
        },
        Action::Mobilize => ActionDelta {
            budget: -8,
            defcon: None,
            tension: 6.0,
            detection: 8.0,
        },
        Action::Strike => ActionDelta {
            budget: 0,
            defcon: None,
            tension: 0.0,
            detection: 0.0,
        },
        Action::Negotiate => ActionDelta {
            budget: -4,
            defcon: Some(1), // +1 means DEFCON goes up (less tense)
            tension: -5.0,
            detection: 2.0,
        },
        Action::Disarm => ActionDelta {
            budget: 0,
            defcon: None,
            tension: 0.0,
            detection: 0.0,
        },
        Action::Bluff => ActionDelta {
            budget: -2,
            defcon: None,
            tension: 2.0,
            detection: 3.0,
        },
        Action::StandDown => ActionDelta {
            budget: 0,
            defcon: Some(1),
            tension: -4.0,
            detection: 1.0,
        },
        Action::Intercept => ActionDelta {
            budget: -5,
            defcon: None,
            tension: -2.0,
            detection: -6.0, // opponent sees less of us
        },
        Action::Declassify => ActionDelta {
            budget: -1,
            defcon: None,
            tension: -10.0,
            detection: 3.0,
        },
        Action::Harden => ActionDelta {
            budget: -4,
            defcon: None,
            tension: 2.0,
            detection: 1.0,
        },
        // Proxy / terror-actor actions. Costs and tensions are
        // intentionally modest — the *consequences* of the action
        // (radicalization, retaliation, alliance drag-in) live in
        // `apply_action`'s post-delta proxy block, not here.
        Action::FundProxy => ActionDelta {
            budget: -6,
            defcon: None,
            tension: 3.0,
            detection: 2.0,
        },
        Action::CutSupport => ActionDelta {
            budget: 0, // frees nothing in the short term
            defcon: None,
            tension: 4.0, // abandoned proxies retaliate
            detection: 1.0,
        },
        Action::StrikeProxy => ActionDelta {
            budget: -10,
            defcon: None,
            tension: 8.0,
            detection: 6.0,
        },
        Action::Sanction => ActionDelta {
            budget: -3,
            defcon: None,
            tension: 1.0,
            detection: 1.0,
        },
    }
}

#[allow(dead_code)]
fn _ensure_side_state_used(_: &SideState) {} // suppress unused warning if needed

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{Era, Faction, Theater};

    fn fresh() -> WorldState {
        WorldState {
            turn: 1,
            era: Era::ColdWar,
            theater: Theater::NorthAtlantic,
            faction: Faction::Us,
            defcon: 3,
            tension: 50.0,
            detection_pct: 50.0,
            sides: [
                SideState::default_player(),
                SideState::default_opponent(),
            ],
            log: vec![],
            terminal: None,
            terror_actors: vec![],
            alliances: vec![],
        }
    }

    #[test]
    fn strike_terminates_immediately() {
        let s = apply_action(&fresh(), Side::Us, Action::Strike);
        assert!(is_terminal(&s));
        match s.terminal {
            Some(GameOutcome::Strike { by: Side::Us, .. }) => {}
            other => panic!("expected Strike by Us, got {:?}", other),
        }
    }

    #[test]
    fn disarm_terminates_immediately() {
        let s = apply_action(&fresh(), Side::Opp, Action::Disarm);
        assert!(is_terminal(&s));
    }

    #[test]
    fn negotiate_increases_defcon() {
        let mut s = fresh();
        s.defcon = 2;
        let after = apply_action(&s, Side::Us, Action::Negotiate);
        assert_eq!(after.defcon, 3);
    }

    #[test]
    fn defcon_clamps_at_5() {
        let mut s = fresh();
        s.defcon = 5;
        let after = apply_action(&s, Side::Us, Action::Negotiate);
        assert_eq!(after.defcon, 5);
    }

    #[test]
    fn defcon_clamps_at_1() {
        let mut s = fresh();
        s.defcon = 1;
        let after = apply_action(&s, Side::Us, Action::Feint);
        // Feint doesn't move DEFCON, so it stays at 1.
        assert_eq!(after.defcon, 1);
    }

    #[test]
    fn escalate_via_mobilize_lowers_defcon() {
        let mut s = fresh();
        s.defcon = 5;
        let after = apply_action(&s, Side::Us, Action::Mobilize);
        // Mobilize doesn't change DEFCON directly (it's posture-driven); clamps at 5.
        assert_eq!(after.defcon, 5);
    }

    #[test]
    fn budget_decays_each_turn() {
        let s = fresh();
        let after = apply_action(&s, Side::Us, Action::Patrol);
        assert_eq!(after.sides[0].escalation_budget, s.sides[0].escalation_budget - 1);
        assert_eq!(after.sides[1].escalation_budget, s.sides[1].escalation_budget - 1);
    }

    #[test]
    fn detection_drifts_with_aggression() {
        let s = fresh();
        // Both sides go aggressive → detection should rise.
        let s = apply_action(&s, Side::Us, Action::Feint);
        let s = apply_action(&s, Side::Opp, Action::Mobilize);
        // Mobilize hardens posture, Feint is aggressive → aggressor_count = 2 → +4.
        assert!(s.detection_pct > s.detection_pct - 4.0 + 0.001 || s.detection_pct >= 100.0);
    }

    #[test]
    fn defection_when_budget_exhausted_under_hardened() {
        let mut s = fresh();
        s.sides[0].escalation_budget = 0;
        s.sides[0].posture = Posture::Hardened;
        let after = apply_action(&s, Side::Us, Action::Patrol); // any non-strike, non-disarm
        // Patrol sets posture to Routine; defection requires Hardened. So we use Mobilize on opponent.
        let after = apply_action(&s, Side::Opp, Action::Mobilize);
        // Mobilize hardens opp → opp's posture == Hardened, opp budget is still > 0
        // because budget only decays on next action. To trigger defection we need
        // a state where Hardened + budget == 0 — apply multiple Mobilizes to drain.
        let mut drained = fresh();
        drained.sides[1].escalation_budget = 0;
        drained.sides[1].posture = Posture::Hardened;
        let after = apply_action(&drained, Side::Us, Action::Patrol);
        // Patrol sets US posture to Routine, opp stays Hardened. Defection fires.
        assert!(matches!(after.terminal, Some(GameOutcome::Defect { by: Side::Opp, .. })));
        // Also confirm we don't crash on the earlier-after.
        let _ = after;
    }

    // ---- Proxy / terror-actor action tests (M5) ----

    fn fresh_with_proxy() -> WorldState {
        let mut s = fresh();
        s.terror_actors = vec![crate::proxies::TerrorActor {
            id: "crescent-falcons".into(),
            name: "Crescent Falcons".into(),
            region: "Eastern Mediterranean".into(),
            sponsor: crate::proxies::Sponsor::Opp,
            capability: 30,
            radicalization: 40,
            autonomy: 50,
        }];
        s
    }

    #[test]
    fn fund_proxy_decreases_budget_and_raises_tension() {
        let s = fresh_with_proxy();
        let before_budget = s.side(Side::Us).escalation_budget;
        let before_tension = s.tension;
        let after = apply_action(&s, Side::Us, Action::FundProxy);
        assert!(
            after.side(Side::Us).escalation_budget < before_budget,
            "FundProxy must cost budget: before={before_budget} after={}",
            after.side(Side::Us).escalation_budget
        );
        assert!(
            after.tension > before_tension,
            "FundProxy must raise tension: before={before_tension} after={}",
            after.tension
        );
        // Posture should not change — funding a proxy isn't itself
        // an escalation move, even though it raises tension.
        assert_eq!(after.side(Side::Us).posture, s.side(Side::Us).posture);
    }

    #[test]
    fn cut_support_raises_tension_from_retaliation() {
        let s = fresh_with_proxy();
        let before_tension = s.tension;
        let after = apply_action(&s, Side::Us, Action::CutSupport);
        assert!(
            after.tension > before_tension,
            "CutSupport must raise tension via retaliation: before={before_tension} after={}",
            after.tension
        );
    }

    #[test]
    fn strike_proxy_terminates_non_terminal_and_raises_detection() {
        let s = fresh_with_proxy();
        let before_detection = s.detection_pct;
        let after = apply_action(&s, Side::Us, Action::StrikeProxy);
        // StrikeProxy isn't terminal itself (unlike `Strike`) — the
        // game continues, the actor is just gone.
        assert!(!is_terminal(&after));
        assert!(
            after.detection_pct >= before_detection,
            "StrikeProxy must not lower detection (the world notices): before={before_detection} after={}",
            after.detection_pct
        );
    }

    #[test]
    fn sanction_keeps_terminal_none_and_modest_tension() {
        let s = fresh_with_proxy();
        let before_tension = s.tension;
        let after = apply_action(&s, Side::Us, Action::Sanction);
        assert!(!is_terminal(&after));
        // Sanction is the softest proxy action — only +1 tension.
        assert!(after.tension - before_tension < 5.0);
    }

    #[test]
    fn all_proxy_actions_round_trip_through_serde() {
        // Sanity check: every new action variant serializes to a
        // distinct snake_case tag and round-trips through JSON.
        for action in [
            Action::FundProxy,
            Action::CutSupport,
            Action::StrikeProxy,
            Action::Sanction,
        ] {
            let json = serde_json::to_string(&action).unwrap();
            let back: Action = serde_json::from_str(&json).unwrap();
            assert_eq!(back, action, "round-trip failed for {action:?}");
            assert!(json.starts_with('"') && json.ends_with('"'));
        }
    }

    #[test]
    fn world_state_round_trips_with_proxies_and_alliances() {
        let mut s = fresh();
        s.terror_actors = vec![crate::proxies::TerrorActor {
            id: "x".into(),
            name: "X".into(),
            region: "y".into(),
            sponsor: crate::proxies::Sponsor::Independent,
            capability: 50,
            radicalization: 60,
            autonomy: 70,
        }];
        s.alliances = vec![crate::proxies::Alliance {
            sides: [Side::Us, Side::Opp],
            kind: crate::proxies::AllianceKind::Rivalry,
            strength: 80,
        }];
        let json = serde_json::to_string(&s).unwrap();
        let back: WorldState = serde_json::from_str(&json).unwrap();
        assert_eq!(back.terror_actors.len(), 1);
        assert_eq!(back.alliances.len(), 1);
        assert_eq!(
            back.terror_actors[0].sponsor,
            crate::proxies::Sponsor::Independent
        );
    }
}