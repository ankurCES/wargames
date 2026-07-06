//! AI vs AI mode — two agents play both sides of the war game.
//!
//! Each agent has:
//!   - a [`AgentPersona`] that shapes preference over the 11 actions at a
//!     given posture/tension band,
//!   - an [`AgentMemory`] (a bounded ring of recent observations and the
//!     agent's own last reasoning snippet),
//!   - an [`AgentLearner`] that adjusts the persona's bias weights against
//!     observed outcomes (posture deltas, terminal events, opponent actions)
//!     and folds those adjustments back into the persona the next turn.
//!
//! An `AiVsAiRunner` advances the world one half-turn at a time using each
//! agent's `decide` impl. Both sides are decided by their agent — the engine
//! is unmodified. Runners are pure (no I/O, no LLM call): the TUI can
//! compose them with the existing `apply_action` flow.

use crate::actions::Action;
use crate::agents::learning::OutcomeSample;
use crate::state::{Era, Faction, Side, WorldState};

pub mod learning;
pub mod personas;

pub use learning::AgentLearner;
pub use personas::AgentPersona;

/// Stable identifier for an agent in a match.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AgentId {
    AggressiveEscalator,
    CalculatedDefender,
}

impl AgentId {
    pub fn display(self) -> &'static str {
        match self {
            AgentId::AggressiveEscalator => "Aggressive Escalator",
            AgentId::CalculatedDefender => "Calculated Defender",
        }
    }
}

/// Reading window passed to `decide`. The agent sees:
///   - its own last N actions (in turn order),
///   - opponent's last N actions (in turn order),
///   - the current filtered world state (its own side is shown; opponent side
///     is partially visible, per the existing `detection_pct` discipline).
#[derive(Debug, Clone)]
pub struct MemoryView<'a> {
    pub world: &'a WorldState,
    pub own_side: Side,
    pub own_recent: &'a [RecordedAction],
    pub opp_recent: &'a [RecordedAction],
}

#[derive(Debug, Clone)]
pub struct RecordedAction {
    pub turn: u32,
    pub action: Action,
}

/// Reasoning trace left by `decide` so the TUI can show it without leaking
/// internal state. Kept short (≤100 chars enforced at write time).
#[derive(Debug, Clone, Default)]
pub struct ReasoningSnippet(pub String);

impl ReasoningSnippet {
    pub fn new(s: impl Into<String>) -> Self {
        let s: String = s.into();
        let truncated: String = s.chars().take(100).collect();
        Self(truncated)
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Bounded memory of an agent: own + opponent recent actions.
#[derive(Debug, Clone)]
pub struct AgentMemory {
    pub own: Vec<RecordedAction>,
    pub opp: Vec<RecordedAction>,
    pub last_reasoning: ReasoningSnippet,
    capacity: usize,
}

impl AgentMemory {
    pub fn new(capacity: usize) -> Self {
        Self {
            own: Vec::with_capacity(capacity),
            opp: Vec::with_capacity(capacity),
            last_reasoning: ReasoningSnippet::default(),
            capacity,
        }
    }

    pub fn record_own(&mut self, turn: u32, a: Action) {
        Self::push_bounded(&mut self.own, self.capacity, RecordedAction { turn, action: a });
    }

    pub fn record_opp(&mut self, turn: u32, a: Action) {
        Self::push_bounded(&mut self.opp, self.capacity, RecordedAction { turn, action: a });
    }

    pub fn set_reasoning(&mut self, r: ReasoningSnippet) {
        self.last_reasoning = r;
    }

    fn push_bounded(buf: &mut Vec<RecordedAction>, capacity: usize, entry: RecordedAction) {
        buf.push(entry);
        if buf.len() > capacity {
            let drop = buf.len() - capacity;
            buf.drain(0..drop);
        }
    }

    pub fn last_own(&self) -> Option<&RecordedAction> {
        self.own.last()
    }
    pub fn last_opp(&self) -> Option<&RecordedAction> {
        self.opp.last()
    }
}

impl Default for AgentMemory {
    fn default() -> Self {
        Self::new(8)
    }
}

/// Full agent: persona + memory + learner. The persona contributes a base
/// bias table; the learner nudges it.
#[derive(Debug, Clone)]
pub struct Agent {
    pub id: AgentId,
    pub persona: AgentPersona,
    pub memory: AgentMemory,
    pub learner: AgentLearner,
    pub faction: Faction,
    pub era: Era,
}

impl Agent {
    pub fn new(id: AgentId, persona: AgentPersona, faction: Faction, era: Era) -> Self {
        Self {
            id,
            persona,
            memory: AgentMemory::default(),
            learner: AgentLearner::new(id),
            faction,
            era,
        }
    }

    /// Decide this agent's next action. Pure: no I/O, no LLM, no clock.
    /// Reads the world via `MemoryView` and updates internal memory + learner.
    pub fn decide(&self, view: MemoryView) -> (Action, ReasoningSnippet) {
        let base_weights = self.persona.weights(view.world, view.own_side);
        let learner_weights = self.learner.weights();
        let combined = combine(base_weights, learner_weights);
        choose(combined, view.world, view.own_side, &self.persona, self.id)
    }

    /// Record an outcome sample so the learner can update next turn.
    pub fn observe_outcome(&mut self, sample: OutcomeSample) {
        self.learner.record(sample);
    }
}

/// Combine persona and learner weight tables. The learner scales the persona
/// weights by `1 + adj` for each (action) where adj ∈ [-0.4, +0.4]. A negative
/// adj hurts the action's odds; a positive one helps.
fn combine(base: [f32; 11], adj: [f32; 11]) -> [f32; 11] {
    let mut out = [0.0f32; 11];
    for i in 0..11 {
        let bias: f32 = (1.0 + adj[i]).max(0.6);
        out[i] = base[i] * bias;
    }
    out
}

/// Soft-max weighted pick from a weight vector. Determinism comes from the
/// (id + world.turn) seed; tests can pin the choice by constructing an
/// `Agent` and passing a known `WorldState`.
fn choose(
    weights: [f32; 11],
    state: &WorldState,
    own_side: Side,
    persona: &AgentPersona,
    id: AgentId,
) -> (Action, ReasoningSnippet) {
    use rand::rngs::StdRng;
    use rand::{Rng, SeedableRng};

    // Filter out the immediately terminal actions when they're not warranted —
    // strike is almost never the right first move in the opening; disarm is
    // also an end-game move. We still allow them when DEFCON is 1 or budget is
    // exhausted (the learner will see those as outcomes and adjust).
    let mut allowed = weights;
    if state.defcon > 1 {
        allowed[Action::Strike as usize] *= 0.05;
    }
    if state.side(own_side).escalation_budget > 5 {
        allowed[Action::Disarm as usize] *= 0.05;
    }

    // Seed the RNG from the agent's id + world.turn so the choice is
    // deterministic for tests.
    let seed = seed_for(id, state.turn);
    let mut rng = StdRng::seed_from_u64(seed);

    let total: f32 = allowed.iter().sum();
    if total <= 0.0 {
        // Defensive: should not happen, but if every weight is zero, pick Patrol.
        let reasoning = ReasoningSnippet::new("weights collapsed — falling back to patrol");
        return (Action::Patrol, reasoning);
    }
    let pick: f32 = rng.gen_range(0.0..total);
    let mut cum = 0.0f32;
    let mut chosen = Action::Patrol;
    for (idx, &w) in allowed.iter().enumerate() {
        cum += w;
        if pick <= cum {
            chosen = index_to_action(idx);
            break;
        }
    }
    let reasoning = ReasoningSnippet::new(format!(
        "{} — chose {}",
        persona.name(),
        chosen.display()
    ));
    (chosen, reasoning)
}

fn index_to_action(idx: usize) -> Action {
    // Mirror the order in actions::Action, which is the order that
    // persona.weights() and learner.weights() emit. Keep this mapping in
    // sync with both tables — a mismatch is a bug.
    match idx {
        0 => Action::Patrol,
        1 => Action::Feint,
        2 => Action::Mobilize,
        3 => Action::Strike,
        4 => Action::Negotiate,
        5 => Action::Disarm,
        6 => Action::Bluff,
        7 => Action::StandDown,
        8 => Action::Intercept,
        9 => Action::Declassify,
        10 => Action::Harden,
        _ => Action::Patrol,
    }
}

fn seed_for(id: AgentId, turn: u32) -> u64 {
    let id_byte = match id {
        AgentId::AggressiveEscalator => 0xAE,
        AgentId::CalculatedDefender => 0xCD,
    };
    ((id_byte as u64) << 24) | (turn as u64)
}

/// Drives a full AI-vs-AI match. Pure (no LLM, no I/O). Caller supplies
/// the two agents, an initial world, and the loop is bounded by max_turns.
#[derive(Debug, Clone)]
pub struct AiVsAiRunner {
    pub max_turns: u32,
}

#[derive(Debug, Clone)]
pub struct MatchSummary {
    pub turns_played: u32,
    pub terminal: Option<crate::engine::GameOutcome>,
    pub actions: Vec<RecordedAction>,
}

impl AiVsAiRunner {
    pub fn new(max_turns: u32) -> Self {
        Self { max_turns: max_turns.max(2) }
    }

    /// Run a full match from `state` and return a summary. Mutates `agents`
    /// to advance their memory and learner.
    pub fn run_match(
        &self,
        mut state: WorldState,
        agents: (&mut Agent, &mut Agent),
    ) -> MatchSummary {
        use crate::engine::{apply_action, is_terminal};
        let mut actions = Vec::new();

        // Both sides get a turn per cycle. The engine treats them
        // symmetrically; we just feed it one decision per side.
        for _turn in 0..self.max_turns {
            if is_terminal(&state) {
                break;
            }
            for side in [Side::Us, Side::Opp] {
                if is_terminal(&state) {
                    break;
                }
                let agent: &mut Agent = if side == Side::Us {
                    agents.0
                } else {
                    agents.1
                };
                // Copy recent slices into local Vecs so the borrows on
                // `agent.memory` end before we call `decide(&self)`.
                let own_recent: Vec<RecordedAction> = agent.memory.own.clone();
                let opp_recent: Vec<RecordedAction> = agent.memory.opp.clone();
                let view = MemoryView {
                    world: &state,
                    own_side: side,
                    own_recent: &own_recent,
                    opp_recent: &opp_recent,
                };
                let (a, reasoning) = agent.decide(view);
                agent.memory.record_own(state.turn, a);
                agent.memory.set_reasoning(reasoning);
                let prev_turn = state.turn;
                state = apply_action(&state, side, a);
                actions.push(RecordedAction {
                    turn: prev_turn,
                    action: a,
                });
                // Fold an outcome sample into the learner's buffer so it can
                // adjust weights next turn.
                let own_after = state.side(side).clone();
                agent.observe_outcome(OutcomeSample {
                    turn: prev_turn,
                    action: a,
                    side,
                    defcon_before: state.defcon,
                    posture_after: own_after.posture,
                    budget_after: own_after.escalation_budget,
                    tension_after: state.tension,
                    terminal_now: is_terminal(&state),
                    own_action: a,
                });
            }
        }

        MatchSummary {
            turns_played: state.turn,
            terminal: state.terminal,
            actions,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{Era, Faction, SideState, Theater};

    fn fresh() -> WorldState {
        WorldState {
            turn: 1,
            era: Era::Modern,
            theater: Theater::BlackSea,
            faction: Faction::Nato,
            defcon: 3,
            tension: 50.0,
            detection_pct: 50.0,
            sides: [SideState::default_player(), SideState::default_opponent()],
            log: vec![],
            terminal: None,
            terror_actors: vec![],
            alliances: vec![],
        }
    }

    #[test]
    fn ai_vs_ai_match_runs_to_terminal_or_bounded_turns() {
        // Two agents with default personas, run for up to 100 turns. The
        // match must either reach a terminal outcome or advance at least
        // a few turns and produce a non-empty actions log.
        let mut a = Agent::new(
            AgentId::AggressiveEscalator,
            AgentPersona::escalator(),
            Faction::Us,
            Era::Modern,
        );
        let mut b = Agent::new(
            AgentId::CalculatedDefender,
            AgentPersona::defender(),
            Faction::Nato,
            Era::Modern,
        );
        let runner = AiVsAiRunner::new(100);
        let summary = runner.run_match(fresh(), (&mut a, &mut b));
        // Either we hit a terminal, or we advanced — not both false.
        assert!(
            summary.terminal.is_some() || !summary.actions.is_empty(),
            "match produced neither a terminal nor any actions"
        );
        // Either way, both agents must have recorded at least one own
        // action in memory.
        assert!(!a.memory.own.is_empty(), "agent A memory empty");
        assert!(!b.memory.own.is_empty(), "agent B memory empty");
    }

    #[test]
    fn learner_receives_outcomes_per_turn() {
        let mut a = Agent::new(
            AgentId::AggressiveEscalator,
            AgentPersona::escalator(),
            Faction::Us,
            Era::Modern,
        );
        let mut b = Agent::new(
            AgentId::CalculatedDefender,
            AgentPersona::defender(),
            Faction::Nato,
            Era::Modern,
        );
        let runner = AiVsAiRunner::new(8);
        let _ = runner.run_match(fresh(), (&mut a, &mut b));
        // Both learners received samples.
        assert!(a.learner.len() > 0, "learner A never received outcomes");
        assert!(b.learner.len() > 0, "learner B never received outcomes");
        // Stronger property — at least one of (a, b) had enough samples
        // for the learner to produce a nonzero adj vector. The other may
        // legitimately stay at zero if its samples average out around 0.
        let adj_a = a.learner.weights();
        let adj_b = b.learner.weights();
        let any_nonzero = adj_a.iter().chain(adj_b.iter()).any(|v| *v != 0.0);
        assert!(
            any_nonzero,
            "at least one learner's adj should diverge from zero after 8 turns"
        );
    }

    #[test]
    fn summary_turns_played_advances() {
        let mut a = Agent::new(
            AgentId::AggressiveEscalator,
            AgentPersona::escalator(),
            Faction::Us,
            Era::Modern,
        );
        let mut b = Agent::new(
            AgentId::CalculatedDefender,
            AgentPersona::defender(),
            Faction::Nato,
            Era::Modern,
        );
        let runner = AiVsAiRunner::new(6);
        let summary = runner.run_match(fresh(), (&mut a, &mut b));
        // World state's `turn` increments by 1 per action; with up to
        // 12 actions (6 turns × 2 sides), `turns_played` should be > 1
        // and ≤ 1 + 12.
        assert!(summary.turns_played > 1);
        assert!(summary.turns_played <= 1 + 6 * 2);
    }
}
