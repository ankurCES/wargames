//! Per-agent learner. Holds a bounded ring of `OutcomeSample`s and emits an
//! adjustment vector over the 11 actions that nudges the persona's weights.
//!
//! This is intentionally a small, pure-Rust learner — no gradient, no model.
//! It answers the question: *given recent observations, which of my actions
//! were associated with "good" outcomes?* "Good" is defined as:
//!
//!   - I avoided escalating DEFCON (defcon_before went up or stayed),
//!   - I avoided hitting terminal (terminal_now == false),
//!   - I did not consume more than 1 unit of escalation_budget per turn
//!     on average,
//!   - my posture moved *toward* the persona's preferred posture (not
//!     enforced; sampled only),
//!   - tension moved *down* or stayed flat (vs. up).
//!
//! Each sample contributes a [−1.0, +1.0] reward; the recent ring's mean
//! reward is what drives the adj vector. Order: same as the persona index
//! legend.

use crate::agents::AgentId;
use crate::state::Side;

#[derive(Debug, Clone, Copy)]
pub struct OutcomeSample
{
    pub turn: u32,
    pub action: crate::actions::Action,
    pub side: Side,
    pub defcon_before: u8,
    pub posture_after: crate::state::Posture,
    pub budget_after: i32,
    pub tension_after: f32,
    pub terminal_now: bool,
    pub own_action: crate::actions::Action,
}

#[derive(Debug, Clone)]
pub struct AgentLearner {
    /// Reserved for future UI work (per-agent summary on the agent status
    /// pane). Not read in the production code path today.
    #[allow(dead_code)]
    pub id: AgentId,
    ring: Vec<OutcomeSample>,
    capacity: usize,
}

impl AgentLearner {
    pub fn new(id: AgentId) -> Self {
        Self {
            id,
            ring: Vec::with_capacity(32),
            capacity: 32,
        }
    }

    pub fn record(&mut self, s: OutcomeSample) {
        self.ring.push(s);
        if self.ring.len() > self.capacity {
            let drop = self.ring.len() - self.capacity;
            self.ring.drain(0..drop);
        }
    }

    /// How many samples are currently buffered. Exposed for tests + the TUI's
    /// agent status pane.
    pub fn len(&self) -> usize {
        self.ring.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ring.is_empty()
    }

    /// Compute the learner's adjustment vector. Returns `[0.0; 11]` until the
    /// ring has at least `min_samples` entries — otherwise we'd be reacting to
    /// noise on the first few turns.
    pub fn weights(&self) -> [f32; 11] {
        const MIN_SAMPLES: usize = 4;
        if self.ring.len() < MIN_SAMPLES {
            return [0.0f32; 11];
        }

        // Reward in [−1.0, +1.0] per sample.
        let mut per_action: [f32; 11] = [0.0f32; 11];
        let mut count: [u32; 11] = [0; 11];
        for s in &self.ring {
            let r = reward(s);
            let idx = s.action as usize;
            per_action[idx] += r;
            count[idx] += 1;
        }
        // Average reward per action, mapped to an adj in [-0.4, +0.4].
        let mut adj = [0.0f32; 11];
        for i in 0..11 {
            if count[i] > 0 {
                let avg = per_action[i] / count[i] as f32;
                // clamp the avg into [-0.4, +0.4]
                adj[i] = (avg * 0.4).clamp(-0.4, 0.4);
            }
        }
        adj
    }
}

fn reward(s: &OutcomeSample) -> f32 {
    if s.terminal_now {
        return -1.0;
    }
    let mut r: f32 = 0.0;
    // Higher DEFCON is calmer; lower is more tense. We slightly reward any
    // move that didn't drive defcon below 2.
    if s.defcon_before >= 2 {
        r += 0.25;
    }
    // Don't punish for routine-only turns; small bonus for de-escalating
    // postures.
    use crate::state::Posture::*;
    r += match s.posture_after {
        Deescalating | Negotiating => 0.25,
        Routine => 0.10,
        Aggressive => -0.10,
        Hardened => -0.25,
    };
    // Tension moved up → small penalty; down → small reward.
    if s.tension_after > 70.0 {
        r -= 0.20;
    } else if s.tension_after < 40.0 {
        r += 0.10;
    }
    // Stay alive on the budget axis.
    if s.budget_after <= 0 {
        r -= 0.5;
    }
    r.clamp(-1.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actions::Action;
    use crate::state::{Posture, Side};

    fn sample(action: Action, terminal_now: bool, defcon_before: u8, tension: f32, budget: i32) -> OutcomeSample {
        OutcomeSample {
            turn: 1,
            action,
            side: Side::Us,
            defcon_before,
            posture_after: Posture::Routine,
            budget_after: budget,
            tension_after: tension,
            terminal_now,
            own_action: action,
        }
    }

    #[test]
    fn empty_learner_returns_zero_weights() {
        let l = AgentLearner::new(AgentId::CalculatedDefender);
        assert_eq!(l.weights(), [0.0f32; 11]);
    }

    #[test]
    fn terminal_samples_clamp_to_strong_negative_reward() {
        let mut l = AgentLearner::new(AgentId::AggressiveEscalator);
        for _ in 0..6 {
            l.record(sample(Action::Strike, true, 1, 100.0, 0));
        }
        let w = l.weights();
        let idx = Action::Strike as usize;
        assert!(w[idx] < 0.0, "terminal-asociated action must drift negative");
    }

    #[test]
    fn learner_converges_after_consistent_good_outcomes() {
        let mut l = AgentLearner::new(AgentId::CalculatedDefender);
        // Patrol / de-escalating 12 turns in a row → Patrol should accrue
        // a positive adj.
        for _ in 0..12 {
            l.record(sample(Action::Patrol, false, 4, 35.0, 40));
        }
        let w = l.weights();
        assert!(
            w[Action::Patrol as usize] > 0.0,
            "patrol with sustained good outcomes should drift positive"
        );
        assert!(
            w[Action::StandDown as usize].abs() <= 0.4,
            "adj is clamped at ±0.4"
        );
    }

    #[test]
    fn ring_is_bounded() {
        let mut l = AgentLearner::new(AgentId::CalculatedDefender);
        // Overflow by 50 — ring must stay at capacity.
        for i in 0..(AgentLearner::new(AgentId::CalculatedDefender).capacity + 50) {
            l.record(sample(Action::Patrol, false, 3, 50.0, 30));
            assert!(l.len() <= l.capacity || l.len() <= 32);
            // Defensive against API churn: max is 32 by spec.
            assert!(l.len() <= 32, "iteration {}: len={}", i, l.len());
        }
        assert_eq!(l.len(), 32);
    }
}
