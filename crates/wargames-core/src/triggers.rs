//! World-event triggers.

use crate::log::LogEntry;
use crate::state::{Era, Theater, WorldState};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TriggerId {
    KaliningradCycle,
    TaiwanAdizBreach,
    SubmarineContact,
    CyberBlink,
}

impl TriggerId {
    pub fn display(self) -> &'static str {
        match self {
            TriggerId::KaliningradCycle => "Kaliningrad cycle — Iskander reload detected",
            TriggerId::TaiwanAdizBreach => "Taiwan ADIZ breach — PLAAF crossing median line",
            TriggerId::SubmarineContact => "Submarine contact — sonar anomaly in GIUK gap",
            TriggerId::CyberBlink => "Cyber blink — power-grid anomaly, attribution unclear",
        }
    }
}

/// A trigger's conditions + the effect when it fires.
#[derive(Debug, Clone, Copy)]
pub struct Trigger {
    pub id: TriggerId,
    pub era: Option<Era>,
    pub theater: Option<Theater>,
    pub tension_gte: Option<f32>,
    pub defcon_lte: Option<u8>,
    pub detection_gte: Option<f32>,
    pub tension_delta: f32,
    pub defcon_delta: i32,
}

const TRIGGERS: &[Trigger] = &[
    Trigger {
        id: TriggerId::KaliningradCycle,
        era: Some(Era::Modern),
        theater: Some(Theater::BalticSea),
        tension_gte: Some(60.0),
        defcon_lte: Some(4),
        detection_gte: None,
        tension_delta: 5.0,
        defcon_delta: -1,
    },
    Trigger {
        id: TriggerId::TaiwanAdizBreach,
        era: None,
        theater: Some(Theater::TaiwanStrait),
        tension_gte: Some(40.0),
        defcon_lte: Some(5),
        detection_gte: None,
        tension_delta: 4.0,
        defcon_delta: -1,
    },
    Trigger {
        id: TriggerId::SubmarineContact,
        era: None,
        theater: Some(Theater::NorthAtlantic),
        tension_gte: None,
        defcon_lte: Some(3),
        detection_gte: Some(70.0),
        tension_delta: 3.0,
        defcon_delta: 0,
    },
    Trigger {
        id: TriggerId::CyberBlink,
        era: Some(Era::NearPeer2030),
        theater: None,
        tension_gte: Some(30.0),
        defcon_lte: Some(4),
        detection_gte: None,
        tension_delta: 2.0,
        defcon_delta: 0,
    },
];

/// Evaluate all triggers against `state` and apply any that fire. Mutates in place.
pub fn evaluate(state: &mut WorldState) {
    for t in TRIGGERS.iter() {
        if !state_already_fired(state, t.id) && conditions_match(state, t) {
            state.tension = (state.tension + t.tension_delta).clamp(0.0, 100.0);
            if t.defcon_delta != 0 {
                state.defcon = (state.defcon as i32 + t.defcon_delta).clamp(1, 5) as u8;
            }
            state.log.push(LogEntry::trigger(state.turn, t.id.display()));
            // Mark this turn so we don't double-fire this turn.
            state
                .log
                .push(LogEntry::outcome(state.turn, format!("trigger::{:?}", t.id)));
        }
    }
}

fn conditions_match(state: &WorldState, t: &Trigger) -> bool {
    if let Some(era) = t.era {
        if state.era != era {
            return false;
        }
    }
    if let Some(theater) = t.theater {
        if state.theater != theater {
            return false;
        }
    }
    if let Some(t0) = t.tension_gte {
        if state.tension < t0 {
            return false;
        }
    }
    if let Some(d) = t.defcon_lte {
        if state.defcon > d {
            return false;
        }
    }
    if let Some(det) = t.detection_gte {
        if state.detection_pct < det {
            return false;
        }
    }
    true
}

fn state_already_fired(state: &WorldState, id: TriggerId) -> bool {
    state.log.iter().rev().take(8).any(|e| {
        e.kind == "outcome" && e.message == format!("trigger::{:?}", id)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{Faction, SideState};

    #[test]
    fn kaliningrad_fires_when_conditions_met() {
        let mut s = WorldState {
            turn: 1,
            era: Era::Modern,
            theater: Theater::BalticSea,
            faction: Faction::Us,
            defcon: 4,
            tension: 70.0,
            detection_pct: 50.0,
            sides: [SideState::default_player(), SideState::default_opponent()],
            log: vec![],
            terminal: None,
            terror_actors: vec![],
            alliances: vec![],
        };
        let before_tension = s.tension;
        evaluate(&mut s);
        assert!(s.tension > before_tension);
    }

    #[test]
    fn trigger_does_not_fire_wrong_theater() {
        let mut s = WorldState {
            turn: 1,
            era: Era::Modern,
            theater: Theater::TaiwanStrait, // wrong theater for Kaliningrad trigger
            faction: Faction::Us,
            defcon: 4,
            tension: 70.0,
            detection_pct: 50.0,
            sides: [SideState::default_player(), SideState::default_opponent()],
            log: vec![],
            terminal: None,
            terror_actors: vec![],
            alliances: vec![],
        };
        let before = s.tension;
        evaluate(&mut s);
        // Kaliningrad requires BalticSea → must not fire. But Taiwan ADIZ
        // trigger (theater=TaiwanStrait, tension>=40) DOES match and adds
        // tension_delta=4. So we assert: Kaliningrad didn't fire (no 5-point
        // jump), but tension can still change because of the Taiwan trigger.
        assert!(s.tension < before + 5.0, "Kaliningrad should not fire here");
    }
}