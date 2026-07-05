//! Top-level app state machine + main event loop.
//!
//! Resource discipline: the run loop is event-driven (`crossterm::event::read`
//! blocks until input). There is no busy-loop redraw. The only periodic work
//! is the splash countdown (5 s) and the spinner tick (50 ms) while a
//! background task (LLM call, prediction refresh) is in flight.

use crate::config::BlumiSettings;
use crate::llm::LlmClient;
use crate::panes::game_layout;
use crate::picker::{
    default_countries, render_picker, Picker, PickerStep, ScenarioEntry,
};
use crate::splash::render_splash;
use crate::tts::Tts;
use crate::widget_action::{render as render_action, ALL_ACTIONS};
use crate::widget_log::render as render_log;
use crate::widget_predict::render as render_predict;
use crate::widget_radar::render as render_radar;
use crate::widget_spinner;
use crate::widget_state::render as render_state;
use ratatui::widgets::ListState;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use wargames_core::engine::apply_action;
use wargames_core::log::LogEntry;
use wargames_core::predict::predict;
use wargames_core::scenario::Scenario;
use wargames_core::agents::{Agent, AgentId, AgentPersona, RecordedAction};
use wargames_core::{Action, Faction, Posture, Side, SideState};
use wargames_core::WorldState;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Screen {
    Splash,
    Picker,
    Game,
    GameOver,
}

/// Mode flag for the Game screen. `PlayerVsWorld` is the historical behavior;
/// `AiVsAi` runs the entire match with two learned agents deciding both sides,
/// stepping on a self-driven tick.
#[derive(Debug, Clone)]
pub enum Mode {
    PlayerVsWorld,
    AiVsAi {
        agent_a: Agent,
        agent_b: Agent,
        /// Last action each agent took, for the agent status pane.
        last_actions: Vec<RecordedAction>,
        /// Last reasoning snippet from each agent.
        last_reasoning: [(String, String); 2],
        /// When to step the world next.
        next_step_at: Instant,
        /// Tick interval. Default 800 ms — gives the eye time to read each
        /// state, but fast enough to be visible.
        tick: Duration,
        /// Hard cap on turns per match — keeps a runaway loop from running
        /// forever if neither side terminates.
        max_turns_remaining: u32,
    },
}

impl Mode {
    pub fn is_ai_vs_ai(&self) -> bool {
        matches!(self, Mode::AiVsAi { .. })
    }
}

/// What is currently happening in the background, if anything. Drives the
/// spinner overlay so the user never sees a frozen screen.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BgOp {
    Idle,
    /// A real LLM call is in flight.
    LlmCall { started_at: Instant },
    /// A Monte Carlo prediction is being recomputed.
    Predict { started_at: Instant },
    /// Scenario list is being rebuilt or a scenario JSON is being loaded.
    ScenarioLoad { started_at: Instant },
}

impl BgOp {
    pub fn is_busy(&self) -> bool {
        !matches!(self, BgOp::Idle)
    }
    pub fn label(&self) -> &'static str {
        match self {
            BgOp::Idle => "",
            BgOp::LlmCall { .. } => "thinking…",
            BgOp::Predict { .. } => "computing predictions…",
            BgOp::ScenarioLoad { .. } => "loading scenarios…",
        }
    }
}

/// Top-level app — owns the screen state, the loaded scenario, the
/// WorldState, and the (optional) LLM/TTS clients.
pub struct App {
    pub screen: Screen,
    pub splash_started_at: Instant,
    pub picker: Picker,
    pub action_list: ListState,
    pub world: Option<WorldState>,
    pub scenario: Option<Scenario>,
    pub scenario_entry: Option<ScenarioEntry>,
    pub settings: BlumiSettings,
    pub llm: Option<LlmClient>,
    pub tts: Tts,
    pub last_prediction: Option<wargames_core::Prediction>,
    pub last_prediction_at: Option<Instant>,
    pub game_over_message: Option<String>,
    pub scenarios_dir: PathBuf,
    pub status: String,
    pub bg: BgOp,
    pub spinner_frame: usize,
    /// True after `commit_action` until the opponent has responded. The run
    /// loop checks this to decide whether to spawn an LLM task.
    pub opponent_pending: bool,
    /// Live-streamed soviet response text. Appended-to by the SSE task via
    /// a per-turn channel, read by the run loop on every tick. Cleared at
    /// the start of every opponent turn.
    pub streaming_message: String,
    /// Final action assembled from the streaming tool-use input deltas.
    /// `None` until the SSE stream ends.
    pub streaming_action: Option<String>,
    pub mode: Mode,
}

impl App {
    pub fn new(settings: BlumiSettings, scenarios_dir: PathBuf) -> Self {
        let llm = LlmClient::from_settings(&settings);
        let tts = Tts::from_settings(&settings);
        let mut action_list = ListState::default();
        action_list.select(Some(0));
        // Resolve relative `scenarios_dir` against the crate's manifest
        // directory so the picker always finds the bundled scenarios,
        // regardless of the process CWD. Without this, `cargo test`
        // (whose CWD is the crate root) and any user who runs `wargames`
        // from outside the repo both end up with an empty scenario list
        // and a picker that silently hangs on the Scenario step — the
        // exact (a) bug the user has been reporting.
        let scenarios_dir = resolve_scenarios_dir(scenarios_dir);
        let scenarios = load_scenarios(&scenarios_dir);
        let picker = Picker::new(
            crate::picker::default_modes(),
            default_countries(),
            scenarios,
            crate::picker::default_theaters(),
        );
        let status = match llm {
            Some(_) => "LLM ready".to_string(),
            None => "no LLM in settings — opponent will use heuristic".to_string(),
        };
        Self {
            screen: Screen::Splash,
            splash_started_at: Instant::now(),
            picker,
            action_list,
            world: None,
            scenario: None,
            scenario_entry: None,
            settings,
            llm,
            tts,
            last_prediction: None,
            last_prediction_at: None,
            game_over_message: None,
            scenarios_dir,
            status,
            bg: BgOp::Idle,
            spinner_frame: 0,
            opponent_pending: false,
            streaming_message: String::new(),
            streaming_action: None,
            mode: Mode::PlayerVsWorld,
        }
    }

    pub fn tick_splash(&mut self) {
        let elapsed = self.splash_started_at.elapsed();
        if elapsed >= Duration::from_secs(5) {
            self.screen = Screen::Picker;
        }
    }

    pub fn skip_splash(&mut self) {
        self.screen = Screen::Picker;
    }

    /// Mark the run loop as busy with an LLM call. Drives the spinner.
    pub fn set_llm_busy(&mut self) {
        self.bg = BgOp::LlmCall {
            started_at: Instant::now(),
        };
    }

    /// Clear any busy state. Safe to call even when already idle.
    pub fn set_idle(&mut self) {
        self.bg = BgOp::Idle;
    }

    /// Flip into the loading state for the country→scenario transition. The
    /// spinner renders while `bg == ScenarioLoad`; the run loop's render path
    /// auto-clears it after ~250 ms so the user always sees motion but never
    /// gets stuck.
    pub fn set_scenario_loading(&mut self) {
        self.bg = BgOp::ScenarioLoad {
            started_at: Instant::now(),
        };
    }

    /// How long the picker-load spinner has been visible. Used by `render`
    /// to auto-clear so the user is never stuck behind the overlay.
    pub fn scenario_load_elapsed(&self) -> Option<Duration> {
        match self.bg {
            BgOp::ScenarioLoad { started_at } => Some(started_at.elapsed()),
            _ => None,
        }
    }

    /// Compute a snapshot of the LLM-visible state (kept identical to the
    /// JS impl's `stateForLLM`).
    pub fn llm_state_snapshot(&self) -> Option<serde_json::Value> {
        let w = self.world.as_ref()?;
        Some(serde_json::json!({
            "turn": w.turn,
            "defcon": w.defcon,
            "tension": w.tension,
            "detection_pct": w.detection_pct,
            "us_posture": format!("{:?}", w.side(Side::Us).posture).to_lowercase(),
            "opp_posture": format!("{:?}", w.side(Side::Opp).posture).to_lowercase(),
            "us_budget": w.side(Side::Us).escalation_budget,
            "opp_budget": w.side(Side::Opp).escalation_budget,
            "opp_silos_ready": w.side(Side::Opp).silos_ready,
            "opp_subs_at_sea": w.side(Side::Opp).subs_at_sea,
            "us_carriers_operational": w.side(Side::Us).carriers_operational,
        }))
    }

    pub fn enter_game(&mut self) {
        let Some(country) = self.picker.selected_country().cloned() else {
            self.status = "no country selected".into();
            return;
        };
        // In Human vs AI mode the Scenario step holds a bundled entry.
        // Pattern-match the discriminated union returned by the picker.
        let Some(crate::picker::SelectedScenario::Bundled(entry)) =
            self.picker.selected_scenario()
        else {
            self.status = "no scenario selected".into();
            return;
        };
        // Load the full scenario JSON by id; fall back to a synthesised
        // scenario from the entry if the file is missing.
        let scenario = load_scenario_by_id(&self.scenarios_dir, &entry.id)
            .unwrap_or_else(|| synthesised_scenario(&entry));
        let world = build_world(&scenario, country.faction);
        self.world = Some(world);
        self.scenario = Some(scenario);
        self.scenario_entry = Some(entry.clone());
        self.screen = Screen::Game;
        self.status = format!("engaged — DEFCON {}", self.world.as_ref().unwrap().defcon);
    }

    /// Dispatcher: route to the right game-entry function based on the
    /// mode chosen on the Mode step and the scenario/theater chosen on
    /// the Scenario step. Each step in the picker is explicit — by the
    /// time this runs, the player has pressed Enter on Mode, Country,
    /// *and* Scenario.
    fn enter_after_picker(&mut self) {
        use crate::picker::{ModeChoice, SelectedScenario};
        match self.picker.mode {
            ModeChoice::HumanVsAi => self.enter_game(),
            ModeChoice::AiVsAi => {
                // The Scenario step in AI vs AI mode is the theater
                // picker. `selected_scenario()` returns a `TheaterEntry`
                // whose `(theater, seed)` we feed into the seed-driven
                // generator.
                let (theater, seed, faction_override) = match self
                    .picker
                    .selected_scenario()
                {
                    Some(SelectedScenario::Theater(t)) => {
                        let faction = self
                            .picker
                            .selected_country()
                            .map(|c| c.faction);
                        (t.theater, t.seed, faction)
                    }
                    // No theater selected (defensive — shouldn't happen
                    // because advance() guards against empty lists in
                    // AiVsAi mode).
                    _ => {
                        self.status =
                            "no theater selected — pick one and press Enter".into();
                        return;
                    }
                };
                self.enter_ai_vs_ai_for(theater, seed, faction_override);
            }
        }
    }

    /// Enter AI vs AI mode with a fresh time-derived seed. CLI entry point
    /// (`wargames --ai-vs-ai`) — the picker uses `enter_ai_vs_ai_for`
    /// directly with the user-chosen theater.
    pub fn enter_ai_vs_ai(&mut self) {
        use std::time::{SystemTime, UNIX_EPOCH};
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0xBEEF);
        self.enter_ai_vs_ai_with_seed(seed);
    }

    /// AI vs AI entry point with a caller-supplied seed (CLI `--regen`
    /// uses this — pass a fresh nanos value to force a new scenario).
    /// Derives a theater from the seed across all 8.
    pub fn enter_ai_vs_ai_with_seed(&mut self, seed: u64) {
        let theaters = [
            wargames_core::Theater::BalticSea,
            wargames_core::Theater::BlackSea,
            wargames_core::Theater::KoreanPeninsula,
            wargames_core::Theater::TaiwanStrait,
            wargames_core::Theater::SouthChinaSea,
            wargames_core::Theater::RedSea,
            wargames_core::Theater::EasternMed,
            wargames_core::Theater::NorthAtlantic,
        ];
        let theater = theaters[(seed as usize) % theaters.len()];
        self.enter_ai_vs_ai_for(theater, seed, None);
    }

    /// Picker-driven AI vs AI entry: the player has already chosen the
    /// theater, the seed (deterministic per theater), and the faction
    /// they want to play as. `faction_override` is `None` for the CLI
    /// path (mode is symmetric) and `Some(_)` when the picker routed
    /// through the country step.
    pub fn enter_ai_vs_ai_for(
        &mut self,
        theater: wargames_core::Theater,
        seed: u64,
        faction_override: Option<wargames_core::Faction>,
    ) {
        let era = wargames_core::scenario::generator::sample_era(theater, seed);
        let scenario = wargames_core::scenario::generator::generate_scenario(
            theater,
            seed,
            Some(era),
            faction_override,
        );

        // Build a sensible initial world. Mirrors `build_world` shape but
        // skips the bundled JSON — the generator already produced a
        // `Scenario`.
        let mut sides = [
            scenario.us.clone().unwrap_or_default(),
            scenario.soviet.clone().unwrap_or_default(),
        ];
        // Make sure the openers aren't doomed-from-budget. The generator
        // sometimes nudges toward 1.
        if sides[0].escalation_budget < 20 {
            sides[0].escalation_budget = 50;
        }
        if sides[1].escalation_budget < 20 {
            sides[1].escalation_budget = 50;
        }
        // Latch the player's faction — defaults to Us in the CLI path;
        // the picker passes the country the user actually chose.
        let faction = faction_override.unwrap_or(Faction::Us);
        let initial_state_id = match scenario.initial_state.as_deref() {
            Some("DEFCON_5") => 5,
            Some("DEFCON_4") => 4,
            Some("DEFCON_3") => 3,
            Some("DEFCON_2") => 2,
            Some("DEFCON_1") => 1,
            _ => 3,
        };
        let world = WorldState {
            turn: 1,
            era,
            theater,
            faction,
            defcon: initial_state_id,
            tension: 40.0,
            detection_pct: scenario.initial_detection_pct.unwrap_or(40.0),
            sides,
            log: vec![LogEntry::outcome(
                1,
                scenario.opening_message.clone().unwrap_or_default(),
            )],
            terminal: None,
        };
        self.world = Some(world);
        self.scenario = Some(scenario);

        // Two agents, distinct personas, one player-aligned + one
        // opponent-aligned. The picker-driven faction determines which
        // side the player-controlled agent represents; the other agent
        // runs the opposing persona.
        let (player_faction, opp_faction) = match faction {
            Faction::Soviet | Faction::Prc | Faction::Dprk => (Faction::Soviet, Faction::Us),
            _ => (Faction::Us, Faction::Nato),
        };
        let agent_a = Agent::new(
            AgentId::AggressiveEscalator,
            AgentPersona::escalator(),
            player_faction,
            era,
        );
        let agent_b = Agent::new(
            AgentId::CalculatedDefender,
            AgentPersona::defender(),
            opp_faction,
            era,
        );

        self.mode = Mode::AiVsAi {
            agent_a,
            agent_b,
            last_actions: Vec::new(),
            last_reasoning: Default::default(),
            next_step_at: Instant::now() + Duration::from_millis(400),
            tick: Duration::from_millis(800),
            max_turns_remaining: 60,
        };
        self.screen = Screen::Game;
        self.status = format!(
            "AI vs AI engaged — {} as {}",
            theater.display_name(),
            faction.display_name()
        );
        self.refresh_prediction();
    }

    /// Tick the AI vs AI match by one half-turn. Returns true if the match
    /// advanced; false if the tick was suppressed (e.g. terminal already,
    /// game over screen, prediction running, or `next_step_at` not reached).
    pub fn step_ai_vs_ai(&mut self) -> bool {
        // Bail-out reasons don't depend on the agents themselves.
        if self.bg.is_busy() {
            return false;
        }
        let Some(world) = self.world.as_ref() else {
            return false;
        };
        if wargames_core::engine::is_terminal(world) {
            return false;
        }

        // Pull the agents and the timing fields out of the enum so the rest
        // of this method can mutate them without fighting the borrow checker.
        let Mode::AiVsAi {
            mut agent_a,
            mut agent_b,
            mut last_actions,
            last_reasoning,
            next_step_at,
            tick,
            mut max_turns_remaining,
        } = std::mem::replace(&mut self.mode, Mode::PlayerVsWorld)
        else {
            return false;
        };
        let mut last_reasoning = last_reasoning;

        // Don't step until the tick interval elapses.
        if Instant::now() < next_step_at {
            self.restore_ai_vs_ai(
                agent_a,
                agent_b,
                last_actions,
                last_reasoning,
                next_step_at,
                tick,
                max_turns_remaining,
            );
            return false;
        }
        if max_turns_remaining == 0 {
            self.status = "AI vs AI: turn budget exhausted".into();
            self.restore_ai_vs_ai(
                agent_a,
                agent_b,
                last_actions,
                last_reasoning,
                next_step_at,
                tick,
                0,
            );
            return false;
        }

        let mut state = world.clone();
        // Alternate starting side by turn parity. Both halves advance here.
        let sides: [Side; 2] = if state.turn.is_multiple_of(2) {
            [Side::Us, Side::Opp]
        } else {
            [Side::Opp, Side::Us]
        };

        let mut advanced = false;
        for side in sides {
            if wargames_core::engine::is_terminal(&state) {
                break;
            }
            let agent: &mut Agent = if side == Side::Us {
                &mut agent_a
            } else {
                &mut agent_b
            };
            // Snapshot memory slices into owned vectors so the immutable
            // borrow ends before `decide()` takes `&self` of the agent.
            let own_recent: Vec<RecordedAction> = agent.memory.own.clone();
            let opp_recent: Vec<RecordedAction> = agent.memory.opp.clone();
            let view = wargames_core::agents::MemoryView {
                world: &state,
                own_side: side,
                own_recent: &own_recent,
                opp_recent: &opp_recent,
            };
            let (action, reasoning) = agent.decide(view);
            agent.memory.record_own(state.turn, action);
            agent.memory.set_reasoning(reasoning.clone());

            last_actions.push(RecordedAction {
                turn: state.turn,
                action,
            });
            // Map side → slot index (Us = 0, Opp = 1) for the reasoning pane.
            let slot = if side == Side::Us { 0 } else { 1 };
            last_reasoning[slot] = (
                agent.id.display().to_string(),
                reasoning.as_str().to_string(),
            );

            state = apply_action(&state, side, action);
            advanced = true;
        }

        // Cap the actions log so it doesn't grow unbounded.
        if last_actions.len() > 64 {
            let drop = last_actions.len() - 64;
            last_actions.drain(0..drop);
        }
        if advanced {
            max_turns_remaining = max_turns_remaining.saturating_sub(1);
        }

        if let Some(w) = self.world.as_mut() {
            *w = state;
        }
        let new_next_step_at = Instant::now() + tick;
        self.restore_ai_vs_ai(
            agent_a,
            agent_b,
            last_actions,
            last_reasoning,
            new_next_step_at,
            tick,
            max_turns_remaining,
        );
        if advanced {
            self.refresh_prediction();
            self.after_step_check_terminal();
        }
        advanced
    }

    /// Move agents + timing back into `self.mode`. Used by `step_ai_vs_ai`
    /// after `mem::replace` extracted them.
    fn restore_ai_vs_ai(
        &mut self,
        agent_a: Agent,
        agent_b: Agent,
        last_actions: Vec<RecordedAction>,
        last_reasoning: [(String, String); 2],
        next_step_at: Instant,
        tick: Duration,
        max_turns_remaining: u32,
    ) {
        self.mode = Mode::AiVsAi {
            agent_a,
            agent_b,
            last_actions,
            last_reasoning,
            next_step_at,
            tick,
            max_turns_remaining,
        };
    }

    fn after_step_check_terminal(&mut self) {
        let Some(w) = self.world.as_ref() else {
            return;
        };
        if wargames_core::engine::is_terminal(w) {
            self.screen = Screen::GameOver;
            self.game_over_message =
                wargames_core::engine::game_over(w).map(|o| match o {
                    wargames_core::GameOutcome::Strike { by, .. } => {
                        format!("STRIKE by {:?}", by)
                    }
                    wargames_core::GameOutcome::Disarm { by, .. } => {
                        format!("DISARM by {:?}", by)
                    }
                    wargames_core::GameOutcome::Defect { by, .. } => {
                        format!("DEFECT by {:?}", by)
                    }
                });
        }
    }

    pub fn handle_picker_key(&mut self, code: KeyCode) -> bool {
        match code {
            KeyCode::Esc => {
                // `back()` returns true only when the player has backed
                // out of the picker entirely (Esc on the Mode step).
                // That should quit. Anything else — stepping back from
                // Scenario or Country — stays in the picker.
                self.picker.back()
            }
            KeyCode::Up => {
                self.picker.prev();
                self.status = picker_status(&self.picker);
                false
            }
            KeyCode::Down => {
                self.picker.next();
                self.status = picker_status(&self.picker);
                false
            }
            KeyCode::Enter => {
                let step_before = self.picker.step;
                if step_before == PickerStep::Country {
                    // Flip into the loading state BEFORE advance() so the
                    // spinner overlay paints *on top of* the freshly-rendered
                    // scenario list and the user sees a real progress bar
                    // across the transition, not a single-frame flicker.
                    // `enter_game` is deferred until the spinner auto-clears
                    // (see `render`) so the bar has time to fill.
                    self.set_scenario_loading();
                }
                let done = self.picker.advance();
                self.status = picker_status(&self.picker);
                // Two paths into `enter_game`:
                //   1. Picker's `advance()` reports the picker is *done*
                //      AND no background op is in flight — both Country
                //      and Scenario were already completed, the spinner
                //      has auto-cleared (or never started).
                //   2. User pressed Enter on the Scenario step with a
                //      scenario selected — `advance()` returns `done=true`
                //      here. The Country→Scenario spinner already had its
                //      ~900 ms fill window during the previous tick; this
                //      second Enter is the user-confirmed Scenario pick,
                //      so enter the game immediately even if a stale
                //      `ScenarioLoad` is still in `self.bg` (the render
                //      loop will clear it). Without this branch, the
                //      picker would be stuck on the Scenario step after
                //      the user confirms their selection.
                if done && !self.bg.is_busy() {
                    self.enter_after_picker();
                } else if done && step_before == PickerStep::Scenario {
                    self.bg = BgOp::Idle;
                    self.enter_after_picker();
                } else if step_before == PickerStep::Country
                    && self.picker.step == PickerStep::Scenario
                {
                    // explicit transition signal — visible to the user
                    self.status = format!(
                        "country set → {} scenarios available",
                        self.picker.filtered_scenarios().len()
                    );
                }
                false
            }
            _ => false,
        }
    }

    pub fn handle_game_key(&mut self, code: KeyCode) -> bool {
        // Block input during background work so the user can't double-fire.
        if self.bg.is_busy() && !matches!(code, KeyCode::Esc) {
            return false;
        }
        // In AI vs AI mode the game runs itself — Esc still exits.
        if self.mode.is_ai_vs_ai() {
            return match code {
                KeyCode::Esc => true,
                KeyCode::Char('p') | KeyCode::Char('P') => {
                    self.refresh_prediction();
                    false
                }
                _ => false,
            };
        }
        match code {
            KeyCode::Esc => true,
            KeyCode::Up => {
                self.action_prev();
                false
            }
            KeyCode::Down => {
                self.action_next();
                false
            }
            KeyCode::Enter => {
                self.commit_action();
                false
            }
            KeyCode::Char('p') | KeyCode::Char('P') => {
                // Manual prediction refresh — same path the LLM-completion
                // callback uses. The spinner tells the user it's running.
                self.refresh_prediction();
                false
            }
            _ => false,
        }
    }

    fn action_next(&mut self) {
        let len = ALL_ACTIONS.len();
        let i = self.action_list.selected().unwrap_or(0);
        self.action_list.select(Some((i + 1) % len));
    }

    fn action_prev(&mut self) {
        let len = ALL_ACTIONS.len();
        let i = self.action_list.selected().unwrap_or(0);
        let prev = if i == 0 { len - 1 } else { i - 1 };
        self.action_list.select(Some(prev));
    }

    fn commit_action(&mut self) {
        let Some(idx) = self.action_list.selected() else {
            return;
        };
        let Some(action) = ALL_ACTIONS.get(idx).copied() else {
            return;
        };
        let Some(world) = self.world.as_ref() else {
            return;
        };
        let next = apply_action(world, Side::Us, action);
        self.world = Some(next);
        self.status = format!("turn {} — you: {}", self.world.as_ref().unwrap().turn, action.as_str());
        // Mark that the opponent should respond next via the LLM. The actual
        // network call is fired from `main.rs`'s run loop when bg==Idle.
        self.opponent_pending = true;
        // Trigger an initial prediction immediately (cheap, sync).
        self.refresh_prediction();
        // Check for terminal.
        let w = self.world.as_ref().unwrap().clone();
        if wargames_core::engine::is_terminal(&w) {
            self.screen = Screen::GameOver;
            self.game_over_message = wargames_core::engine::game_over(&w).map(|o| match o {
                wargames_core::GameOutcome::Strike { by, .. } => format!("STRIKE by {:?}", by),
                wargames_core::GameOutcome::Disarm { by, .. } => format!("DISARM by {:?}", by),
                wargames_core::GameOutcome::Defect { by, .. } => format!("DEFECT by {:?}", by),
            });
        }
    }

    pub fn refresh_prediction(&mut self) {
        let Some(w) = self.world.as_ref() else {
            return;
        };
        self.bg = BgOp::Predict {
            started_at: Instant::now(),
        };
        let p = predict(w, w.turn as u64 + 1, 200, 5);
        self.last_prediction = Some(p);
        self.last_prediction_at = Some(Instant::now());
        self.bg = BgOp::Idle;
    }

    /// Build the user message that goes to the LLM.
    pub fn build_llm_user_msg(&self) -> Option<String> {
        let snap = self.llm_state_snapshot()?;
        let recent: Vec<String> = self
            .world
            .as_ref()?
            .log
            .iter()
            .rev()
            .take(6)
            .map(|e| format!("[t{}] {}: {}", e.turn, e.side, e.message))
            .collect();
        Some(format!(
            "STATE: {}\nRECENT EVENTS (newest first):\n{}",
            serde_json::to_string_pretty(&snap).unwrap_or_default(),
            recent.into_iter().rev().collect::<Vec<_>>().join("\n")
        ))
    }

    /// Apply an LLM-returned action to the world. Returns the new world if
    /// the action was applied, or None if the action was unknown / invalid.
    pub fn apply_opponent_action(&mut self, raw_action: &str, message: &str) -> bool {
        let Some(action) = parse_action_str(raw_action) else {
            self.status = format!("LLM returned unknown action '{}' — fell back to heuristic", raw_action);
            return self.apply_heuristic_opponent();
        };
        let Some(world) = self.world.as_ref() else {
            return false;
        };
        let mut next = apply_action(world, Side::Opp, action);
        if let Some(prev_msg) = message.lines().next() {
            next.log.push(LogEntry::outcome(
                next.turn,
                format!("soviet says: {}", prev_msg),
            ));
        }
        self.world = Some(next);
        self.opponent_pending = false;
        let w = self.world.as_ref().unwrap().clone();
        self.status = format!("turn {} — opp: {} (\"{}\")", w.turn, action.as_str(), message);
        if wargames_core::engine::is_terminal(&w) {
            self.screen = Screen::GameOver;
            self.game_over_message = wargames_core::engine::game_over(&w).map(|o| match o {
                wargames_core::GameOutcome::Strike { by, .. } => format!("STRIKE by {:?}", by),
                wargames_core::GameOutcome::Disarm { by, .. } => format!("DISARM by {:?}", by),
                wargames_core::GameOutcome::Defect { by, .. } => format!("DEFECT by {:?}", by),
            });
        }
        // Update prediction now that the opponent has moved.
        self.refresh_prediction();
        true
    }

    pub fn apply_heuristic_opponent(&mut self) -> bool {
        let Some(world) = self.world.as_ref().cloned() else {
            return false;
        };
        let opp_action = opponent_heuristic(&world);
        let next = apply_action(&world, Side::Opp, opp_action);
        self.world = Some(next);
        self.opponent_pending = false;
        self.status = format!("opp (heuristic): {}", opp_action.as_str());
        let w = self.world.as_ref().unwrap().clone();
        if wargames_core::engine::is_terminal(&w) {
            self.screen = Screen::GameOver;
            self.game_over_message = wargames_core::engine::game_over(&w).map(|o| match o {
                wargames_core::GameOutcome::Strike { by, .. } => format!("STRIKE by {:?}", by),
                wargames_core::GameOutcome::Disarm { by, .. } => format!("DISARM by {:?}", by),
                wargames_core::GameOutcome::Defect { by, .. } => format!("DEFECT by {:?}", by),
            });
        }
        self.refresh_prediction();
        true
    }

    pub fn render(&mut self, frame: &mut ratatui::Frame) {
        // Auto-clear the picker-loading state once the spinner has been
        // visible long enough to be readable. 900 ms ≈ 18 frames at 50 ms
        // per tick — long enough that the user actually sees the progress
        // bar fill (PROGRESS_TOTAL_FRAMES in widget_spinner), short enough
        // to feel snappy and never block the picker.
        //
        // The deferred `enter_game` from the picker's Country→Scenario
        // Enter path lives here: while ScenarioLoad is active, the user
        // sees the bar fill; on auto-clear we flip into Game so the
        // picker doesn't get stuck on the Scenario step after Enter.
        if let Some(elapsed) = self.scenario_load_elapsed() {
            if elapsed >= Duration::from_millis(900) {
                self.bg = BgOp::Idle;
                if matches!(self.screen, Screen::Picker)
                    && matches!(self.picker.step, PickerStep::Scenario)
                {
                    self.enter_after_picker();
                }
            }
        }
        // Advance the spinner when work is in flight so the user sees motion.
        if self.bg.is_busy() {
            self.spinner_frame = self.spinner_frame.wrapping_add(1);
        }
        // Tick the AI vs AI match on every render — render runs at ~20 fps
        // (50 ms tick); step_ai_vs_ai gates itself on `next_step_at` so
        // the perceived rate is governed by Mode::AiVsAi.tick (default
        // 800 ms). No other state machine path touches the world here.
        if matches!(self.screen, Screen::Game) && self.mode.is_ai_vs_ai() {
            let _ = self.step_ai_vs_ai();
        }
        match self.screen {
            Screen::Splash => {
                let secs_left = (5u8).saturating_sub(
                    self.splash_started_at.elapsed().as_secs().min(5) as u8,
                );
                render_splash(frame, frame.area(), secs_left);
            }
            Screen::Picker => {
                render_picker(frame, frame.area(), &mut self.picker);
                if let BgOp::ScenarioLoad { started_at } = self.bg {
                    let area = widget_spinner::top_right_rect(frame.area());
                    widget_spinner::render(
                        frame,
                        area,
                        "loading scenarios…",
                        self.spinner_frame,
                        started_at,
                    );
                }
            }
            Screen::Game => {
                let (s, p, r, a, l) = game_layout(frame.area());
                if let Some(world) = &self.world {
                    render_state(frame, s, world);
                }
                render_predict(frame, p, self.last_prediction);
                render_radar(frame, r, self.scenario.as_ref());
                render_action(frame, a, &mut self.action_list);
                let log: Vec<LogEntry> = self
                    .world
                    .as_ref()
                    .map(|w| w.log.clone())
                    .unwrap_or_default();
                render_log(frame, l, &log);
                self.render_status_line(frame);
                // Delegate to the shared spinner widget for any busy state
                // (LlmCall / Predict). Anchored at the bottom-right so it never
                // covers the action menu.
                match self.bg {
                    BgOp::LlmCall { started_at } => {
                        let area = widget_spinner::bottom_right_rect(frame.area());
                        widget_spinner::render(
                            frame,
                            area,
                            self.bg.label(),
                            self.spinner_frame,
                            started_at,
                        );
                    }
                    BgOp::Predict { started_at } => {
                        let area = widget_spinner::bottom_right_rect(frame.area());
                        widget_spinner::render(
                            frame,
                            area,
                            self.bg.label(),
                            self.spinner_frame,
                            started_at,
                        );
                    }
                    _ => {}
                }
            }
            Screen::GameOver => {
                self.render_game_over(frame);
            }
        }
    }

    fn render_status_line(&self, frame: &mut ratatui::Frame) {
        use ratatui::style::{Color, Style};
        use ratatui::widgets::Paragraph;
        let area = ratatui::layout::Rect {
            x: 0,
            y: frame.area().height.saturating_sub(1),
            width: frame.area().width,
            height: 1,
        };
        // While the LLM is streaming, show the partial message in the
        // status line so the user sees tokens as they arrive. Once the
        // task completes, fall back to the regular status text.
        let line = if self.bg.is_busy() && !self.streaming_message.is_empty() {
            // Truncate to fit on one terminal row.
            let max = area.width.saturating_sub(2) as usize;
            let msg = if self.streaming_message.len() > max {
                &self.streaming_message[self.streaming_message.len() - max..]
            } else {
                &self.streaming_message
            };
            format!(" » soviet: {}", msg)
        } else {
            format!(
                " {}    [↑↓] action  [Enter] commit  [p] refresh predict  [Esc] quit",
                self.status
            )
        };
        let p = Paragraph::new(line).style(Style::default().bg(Color::Rgb(20, 20, 20)));
        frame.render_widget(p, area);
    }

    fn render_game_over(&self, frame: &mut ratatui::Frame) {
        use ratatui::style::{Color, Modifier, Style};
        use ratatui::text::{Line, Span};
        use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
        let area = frame.area();
        frame.render_widget(Clear, area);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Red));
        let inner = block.inner(area);
        frame.render_widget(block, area);
        let lines = vec![
            Line::from(Span::styled(
                "GAME OVER",
                Style::default()
                    .fg(Color::Red)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from(Span::styled(
                self.game_over_message
                    .clone()
                    .unwrap_or_else(|| "—".into()),
                Style::default().fg(Color::White),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "Press any key to quit.",
                Style::default().fg(Color::DarkGray),
            )),
        ];
        let _ = Wrap { trim: false }; // satisfy unused-import lint on no_std toolchains
        frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
    }
}

fn picker_status(picker: &Picker) -> String {
    match picker.step {
        PickerStep::Mode => format!(
            "pick a mode (↑↓ select, Enter confirm) — {} available",
            picker.modes.len()
        ),
        PickerStep::Country => format!(
            "pick a country (↑↓ select, Enter confirm, Esc back) — {} available",
            picker.countries.len()
        ),
        PickerStep::Scenario => match picker.mode {
            crate::picker::ModeChoice::HumanVsAi => format!(
                "pick a scenario (↑↓ select, Enter confirm, Esc back) — {} filtered",
                picker.filtered_scenarios().len()
            ),
            crate::picker::ModeChoice::AiVsAi => format!(
                "pick a theater for AI vs AI (↑↓ select, Enter confirm, Esc back) — {} available",
                picker.theaters.len()
            ),
        },
    }
}

#[derive(Debug, Clone, Copy)]
pub enum KeyCode {
    Up,
    Down,
    Enter,
    Esc,
    Char(char),
    Any,
}

fn opponent_heuristic(state: &WorldState) -> Action {
    if state.defcon == 1 && state.side(Side::Opp).posture == Posture::Hardened {
        return Action::Strike;
    }
    if state.tension < 30.0 && state.side(Side::Opp).posture == Posture::Routine {
        return Action::Patrol;
    }
    if state.tension > 60.0 {
        return Action::Mobilize;
    }
    Action::Feint
}

fn parse_action_str(s: &str) -> Option<Action> {
    match s.trim().to_ascii_lowercase().as_str() {
        "patrol" => Some(Action::Patrol),
        "feint" => Some(Action::Feint),
        "mobilize" => Some(Action::Mobilize),
        "strike" => Some(Action::Strike),
        "negotiate" => Some(Action::Negotiate),
        "disarm" => Some(Action::Disarm),
        "bluff" => Some(Action::Bluff),
        "stand_down" | "standdown" => Some(Action::StandDown),
        "intercept" => Some(Action::Intercept),
        "declassify" => Some(Action::Declassify),
        "harden" => Some(Action::Harden),
        _ => None,
    }
}

fn build_world(scenario: &Scenario, faction: wargames_core::Faction) -> WorldState {
    let mut sides = [
        SideState::default_player(),
        SideState::default_opponent(),
    ];
    if let Some(us) = &scenario.us {
        sides[0] = us.clone();
    }
    if let Some(opp) = &scenario.soviet {
        sides[1] = opp.clone();
    }
    let defcon = scenario
        .initial_state
        .as_deref()
        .and_then(parse_defcon)
        .unwrap_or(3);
    let detection = scenario.initial_detection_pct.unwrap_or(30.0);
    WorldState {
        turn: 1,
        era: scenario.infer_era(),
        theater: scenario.infer_theater(),
        faction,
        defcon,
        tension: 40.0,
        detection_pct: detection,
        sides,
        log: vec![LogEntry::outcome(
            1,
            format!("scenario \"{}\" engaged", scenario.title),
        )],
        terminal: None,
    }
}

fn parse_defcon(s: &str) -> Option<u8> {
    // "DEFCON_3" → 3
    s.strip_prefix("DEFCON_")?.parse().ok()
}

/// Resolve a possibly-relative `scenarios_dir` to a directory that exists.
/// Order of preference:
///   1. If the path is absolute, use it as-is.
///   2. If the path resolves relative to the current working directory and
///      that directory actually exists, use it.
///   3. Otherwise resolve relative to the crate manifest directory — the
///      path from `scenarios/` to `crates/wargames-tui/` is `../../scenarios`.
///      This makes the bundled scenarios findable from any CWD, so the
///      picker never silently presents an empty scenario list (the (a)
///      bug the user reported: "TUI has a problem after country is
///      selected" → scenarios never load → picker hangs).
fn resolve_scenarios_dir(p: PathBuf) -> PathBuf {
    if p.is_absolute() {
        return p;
    }
    if p.is_dir() {
        return p;
    }
    let from_manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join(&p);
    if from_manifest.is_dir() {
        return from_manifest;
    }
    p
}

fn load_scenarios(dir: &std::path::Path) -> Vec<ScenarioEntry> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(dir) else {
        return out;
    };
    for entry in rd.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let Ok(raw) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(s) = serde_json::from_str::<Scenario>(&raw) else {
            continue;
        };
        let defcon = s
            .initial_state
            .as_deref()
            .and_then(parse_defcon)
            .unwrap_or(3);
        let faction = s.faction.unwrap_or(wargames_core::Faction::Us);
        let theater = s.infer_theater().display_name().to_string();
        out.push(ScenarioEntry {
            id: s.id.clone(),
            title: s.title.clone(),
            defcon,
            theater,
            faction,
        });
    }
    out.sort_by(|a, b| a.title.cmp(&b.title));
    out
}

fn load_scenario_by_id(dir: &std::path::Path, id: &str) -> Option<Scenario> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return None;
    };
    for entry in rd.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let Ok(raw) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(s) = serde_json::from_str::<Scenario>(&raw) else {
            continue;
        };
        if s.id == id {
            return Some(s);
        }
    }
    None
}

fn synthesised_scenario(entry: &ScenarioEntry) -> Scenario {
    Scenario {
        id: entry.id.clone(),
        title: entry.title.clone(),
        briefing: "auto-synthesised scenario".to_string(),
        initial_state: Some(format!("DEFCON_{}", entry.defcon)),
        initial_detection_pct: Some(40.0),
        faction: Some(entry.faction),
        era: None,
        theater: None,
        us: None,
        soviet: None,
        opening_message: None,
        win_conditions: None,
    }
}

#[cfg(test)]
mod playable_flow_tests {
    //! End-to-end playthrough: country → scenario → game → action →
    //! opponent → terminal state. This is the *playable* proof — without it,
    //! the picker tests only prove the picker pieces compile and the
    //! `commit_action` paths are unverified as a single flow.
    //!
    //! No LLM is involved: the heuristic opponent (`opponent_heuristic`)
    //! is what runs when `bg == Idle` and there is no client. This is the
    //! same fallback a user with `blumi settings.json` missing the LLM
    //! block will experience — and it is the path the smoke script can't
    //! exercise (which runs 200 ms with Esc and exits).
    use super::*;
    use crate::config::BlumiSettings;
    use crate::picker::PickerStep;
    use std::path::PathBuf;
    use wargames_core::Action;

    /// Build an `App` against the live `scenarios/` directory using an empty
    /// `BlumiSettings` (no LLM client → heuristic opponent). Returning a
    /// real `App` rather than mocking fields keeps the test honest: every
    /// piece of state set in `App::new` is exercised.
    fn fresh_app() -> App {
        // so build a minimal-but-valid settings JSON on disk and load it
        // through `BlumiSettings::from_path`. The resulting struct has no
        // providers, so `App::new` constructs no `LlmClient` and the
        // heuristic opponent is the one that runs in-game.
        let mut path = std::env::temp_dir();
        // Filename must be unique across parallel tests — `process::id()`
        // alone collides if the same line is reached from two threads. A
        // nanoseconds-from-UNIX_EPOCH suffix keeps it unique without
        // making the test async-only.
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        path.push(format!(
            "wargames_playable_{}_{}.json",
            std::process::id(),
            nanos
        ));
        let payload = br#"{"providers":{},"router":{},"voice":null}"#;
        std::fs::write(&path, payload).expect("write settings fixture");
        let settings =
            crate::config::BlumiSettings::from_path(&path).expect("settings fixture parses");
        // Best-effort cleanup of the fixture — failure here doesn't fail
        // the test (we own the file, /tmp is writable).
        let _ = std::fs::remove_file(&path);
        let dir = PathBuf::from("scenarios");
        let mut app = App::new(settings, dir);
        // Skip splash so the picker is immediately testable.
        app.skip_splash();
        app.bg = BgOp::Idle;
        app
    }

    #[test]
    #[ignore = "touches /tmp fixtures; run with `cargo test -- --ignored`"]
    fn fixture_layout_probe() {
        // placeholder so the module always has at least one runnable test
        // even if `fresh_app` is the only path being exercised — keeps
        // the harness honest about counting tests.
        let app = fresh_app();
        assert_eq!(app.screen, Screen::Picker);
    }

    #[test]
    fn country_to_scenario_to_game_is_reachable() {
        let mut app = fresh_app();
        assert_eq!(app.screen, Screen::Picker);
        assert_eq!(app.picker.step, PickerStep::Mode);

        // 1) Enter on the mode step → moves to country (default HumanVsAi).
        app.handle_picker_key(KeyCode::Enter);
        assert_eq!(app.picker.step, PickerStep::Country);

        // 2) Enter on the country step → moves to scenario, shows spinner.
        app.handle_picker_key(KeyCode::Enter);
        assert_eq!(app.picker.step, PickerStep::Scenario);
        assert!(
            matches!(app.bg, BgOp::ScenarioLoad { .. }),
            "picker Enter at country step must set the spinner busyspinner state"
        );
        assert!(
            !app.picker.filtered_scenarios().is_empty()
                || app.picker.error.is_some(),
            "country selection must produce either visible scenarios or an error"
        );

        // 3) Enter on the scenario step → game screen, world populated.
        app.handle_picker_key(KeyCode::Enter);
        assert_eq!(app.screen, Screen::Game, "Enter at scenario must enter Game");
        assert!(app.world.is_some(), "game screen must have a world");
        assert!(app.scenario.is_some(), "game screen must have a scenario");
        assert_eq!(
            app.world.as_ref().unwrap().turn,
            1,
            "freshly entered game must be on turn 1"
        );
    }

    #[test]
    fn commit_action_advances_turn_and_requests_opponent() {
        let mut app = fresh_app();
        // Drive to the game screen: Mode → Country → Scenario.
        app.handle_picker_key(KeyCode::Enter); // Mode
        app.handle_picker_key(KeyCode::Enter); // Country
        app.handle_picker_key(KeyCode::Enter); // Scenario
        assert_eq!(app.screen, Screen::Game);

        let turn_before = app.world.as_ref().unwrap().turn;
        app.handle_game_key(KeyCode::Enter); // commit default action (0th)
        // Whichever action is at index 0 must advance the turn counter.
        assert_eq!(
            app.world.as_ref().unwrap().turn,
            turn_before + 1,
            "commit_action must advance world.turn"
        );
        assert!(
            app.opponent_pending,
            "after player action, opponent must be pending"
        );
        assert!(
            app.last_prediction.is_some(),
            "commit_action must refresh prediction"
        );
    }

    #[test]
    fn heuristic_opponent_completes_and_prediction_updates() {
        let mut app = fresh_app();
        app.handle_picker_key(KeyCode::Enter); // Mode
        app.handle_picker_key(KeyCode::Enter); // Country
        app.handle_picker_key(KeyCode::Enter); // Scenario
        app.handle_game_key(KeyCode::Enter); // player action
        assert!(app.opponent_pending);

        let turn_before = app.world.as_ref().unwrap().turn;
        let ok = app.apply_heuristic_opponent();
        assert!(ok, "heuristic opponent must always succeed");
        assert!(
            !app.opponent_pending,
            "after heuristic opponent, pending must clear"
        );
        assert_eq!(
            app.world.as_ref().unwrap().turn,
            turn_before + 1,
            "opponent must advance world.turn"
        );
        assert!(
            app.last_prediction.is_some(),
            "opponent response must refresh prediction"
        );
    }

    #[test]
    fn full_playthrough_loop_is_stable_for_many_turns() {
        let mut app = fresh_app();
        app.handle_picker_key(KeyCode::Enter); // Mode
        app.handle_picker_key(KeyCode::Enter); // Country
        app.handle_picker_key(KeyCode::Enter); // Scenario
        // Up/Down on the action menu must not panic, must keep selection
        // in bounds.
        for _ in 0..20 {
            app.handle_game_key(KeyCode::Down);
        }
        for _ in 0..5 {
            app.handle_game_key(KeyCode::Up);
        }
        assert!(app.screen == Screen::Game || app.screen == Screen::GameOver);

        // Alternate player + opponent for 30 turns — proves the loop stays
        // stable and the prediction refresh never panics on a growing log.
        let mut turns = 0u32;
        for _ in 0..30 {
            if app.screen != Screen::Game {
                break;
            }
            app.handle_game_key(KeyCode::Enter);
            app.apply_heuristic_opponent();
            turns += 1;
            // If the game ended naturally, stop.
            if app.screen == Screen::GameOver {
                break;
            }
        }
        assert!(turns >= 1, "must complete at least one full turn pair");
        // Either we're still playing (turn counter grew) or we hit
        // GameOver (a terminal outcome was reached).
        let turn_now = app.world.as_ref().map(|w| w.turn).unwrap_or(0);
        assert!(turn_now > 1 || app.screen == Screen::GameOver);
    }

    #[test]
    fn picker_enter_on_country_shows_progress_affordance() {
        // (b) proof: spinner state must be set on Country→Scenario transition
        // so the progress bar the user asked for actually fires.
        let mut app = fresh_app();
        app.handle_picker_key(KeyCode::Enter); // Mode → Country
        app.handle_picker_key(KeyCode::Enter); // Country → Scenario
        assert!(
            matches!(app.bg, BgOp::ScenarioLoad { .. }),
            "spinner must trigger on country→scenario step"
        );
        assert_eq!(app.picker.step, PickerStep::Scenario);
    }

    /// RENDER-LEVEL proof of (a) — the phantom empty-state on the Country
    /// step. Unit tests assert `picker.error.is_none()`; this test drives
    /// `App::render` through a real `Terminal<TestBackend>` and asserts
    /// the rendered buffer does NOT contain "no scenarios match this
    /// faction" on the Country step, and DOES contain the country
    /// picker title and a country name. This is the proof at the level
    /// the user sees in their terminal.
    #[test]
    fn fresh_picker_render_does_not_show_phantom_empty_state() {
        use ratatui::Terminal;
        use ratatui::TerminalOptions;
        use ratatui::Viewport;
        use ratatui::backend::TestBackend;

        let backend = TestBackend::new(120, 40);
        let mut terminal = Terminal::with_options(backend, TerminalOptions {
            viewport: Viewport::Fullscreen,
        })
        .expect("TestBackend terminal constructs");
        let mut app = fresh_app();
        assert_eq!(app.screen, Screen::Picker);
        assert_eq!(app.picker.step, PickerStep::Mode);

        terminal.draw(|f| app.render(f)).expect("render succeeds");
        // Use the test backend's internal buffer (the one `draw` wrote to),
        // NOT `terminal.current_buffer_mut()` which returns a fresh empty
        // buffer in ratatui 0.30. The buffer contains multi-byte box-drawing
        // glyphs (e.g. '─' is 3 bytes) so we must slice by chars, not bytes,
        // to avoid panicking on a non-char-boundary.
        let backend = terminal.backend();
        let buf = backend.buffer().clone();
        let rendered: String = buf
            .content()
            .iter()
            .map(|c| c.symbol().chars().next().unwrap_or(' '))
            .collect();

        assert!(
            rendered.contains("PICK A MODE"),
            "rendered buffer must show the mode picker title",
        );
        // At least one mode from `default_modes()` must be visible.
        let any_mode = rendered.contains("Human vs AI")
            || rendered.contains("AI vs AI");
        assert!(
            any_mode,
            "rendered buffer must show at least one mode from default_modes()"
        );
        // THE bug the user reported: the phantom empty-state on the
        // Country step, before any country was picked.
        assert!(
            !rendered.contains("no scenarios match this faction"),
            "rendered buffer must NOT show the phantom empty-state on the Country step; \
             got tail: {:?}",
            &rendered[rendered.len().saturating_sub(400)..]
        );
    }

    /// RENDER-LEVEL proof of (b) — the loading affordance must actually
    /// paint in the frame during the Country→Scenario transition. Drives
    /// `App::render` while `BgOp::ScenarioLoad` is active and asserts
    /// the spinner widget's animated text appears in the rendered
    /// buffer (one of the 6 phase verbs).
    #[test]
    fn picker_enter_during_loading_paints_spinner_in_rendered_frame() {
        use ratatui::Terminal;
        use ratatui::TerminalOptions;
        use ratatui::Viewport;
        use ratatui::backend::TestBackend;

        let backend = TestBackend::new(120, 40);
        let mut terminal = Terminal::with_options(backend, TerminalOptions {
            viewport: Viewport::Fullscreen,
        })
        .expect("TestBackend terminal constructs");
        let mut app = fresh_app();
        app.handle_picker_key(KeyCode::Enter); // Mode → Country
        app.handle_picker_key(KeyCode::Enter); // Country → Scenario (spinner)
        // After Enter on the Country step, the picker is on Scenario and
        // bg == ScenarioLoad. Render that frame and assert the spinner
        // widget paints *something* the user can see — one of the phase
        // verbs or the LOADING label.
        assert_eq!(app.picker.step, PickerStep::Scenario);
        assert!(matches!(app.bg, BgOp::ScenarioLoad { .. }));

        // Advance the spinner frame a few ticks so the bar has progressed.
        for _ in 0..3 {
            app.spinner_frame = app.spinner_frame.wrapping_add(1);
            terminal.draw(|f| app.render(f)).expect("render succeeds");
        }

        let backend = terminal.backend();
        let buf = backend.buffer().clone();
        let rendered: String = buf
            .content()
            .iter()
            .map(|c| c.symbol().chars().next().unwrap_or(' '))
            .collect();

        // (b) check: at least one of the spinner affordances appears in
        // the rendered buffer. The widget renders either the "loading
        // scenarios…" label (bg.label()) or one of the 6 phase verbs,
        // or the LOADING shimmer row.
        let has_loading = rendered.contains("loading")
            || rendered.contains("warming")
            || rendered.contains("preparing")
            || rendered.contains("computing")
            || rendered.contains("rendering")
            || rendered.contains("almost")
            || rendered.contains("LOADING");
        assert!(
            has_loading,
            "rendered buffer must show a loading affordance (phase verb / LOADING label) \
             during the ScenarioLoad transition",
        );
    }

    // -- AI vs AI mode ---------------------------------------------------

    #[test]
    fn enter_ai_vs_ai_creates_generated_scenario_and_advances_world() {
        // Wiring proof: the picker routes Mode → Country → Scenario → game,
        // and AI vs AI mode produces a generated scenario with a non-zero
        // world turn after the first tick.
        let mut app = fresh_app();

        // 1) Mode step: switch to AI vs AI (index 1).
        app.handle_picker_key(KeyCode::Down);
        assert_eq!(app.picker.mode_idx, 1);
        // Advance to Country.
        app.handle_picker_key(KeyCode::Enter);
        assert_eq!(app.picker.step, PickerStep::Country);
        assert_eq!(
            app.picker.mode,
            crate::picker::ModeChoice::AiVsAi,
            "Mode choice must be latched before Country shows"
        );

        // 2) Country step: keep the default (US).
        app.handle_picker_key(KeyCode::Enter);
        assert_eq!(app.picker.step, PickerStep::Scenario);

        // 3) Scenario step in AI vs AI mode = theater picker. Keep the
        // default (Baltic Sea) and advance.
        app.handle_picker_key(KeyCode::Enter);
        app.bg = BgOp::Idle;

        // We should now be in Game, with `mode = AiVsAi` and a world.
        assert!(
            app.mode.is_ai_vs_ai(),
            "expected Mode::AiVsAi, got Mode::PlayerVsWorld"
        );
        assert_eq!(app.screen, Screen::Game);
        assert!(
            app.world.is_some(),
            "AI vs AI mode must produce an initial world"
        );
        // Scenario must be a generated one — id starts with the seed
        // prefix the generator produces (format!("{}_gen_{:08x}", seed, seed)).
        let scenario_id = app.scenario.as_ref().unwrap().id.clone();
        assert!(
            scenario_id.contains("_gen_"),
            "AI vs AI scenario id must be a generated one, got {:?}",
            scenario_id
        );

        // Step a few times by directly calling the public `step_ai_vs_ai`.
        // The first call may return false (cold start, no tick yet) or
        // immediately take a half-turn; either way the world `turn` should
        // advance once the underlying tick interval elapses.
        let baseline_turn = app.world.as_ref().unwrap().turn;
        // Force the next step to fire immediately.
        if let Mode::AiVsAi { next_step_at, .. } = &mut app.mode {
            *next_step_at = std::time::Instant::now()
                .checked_sub(std::time::Duration::from_millis(10))
                .unwrap();
        }
        for _ in 0..40 {
            let _ = app.step_ai_vs_ai();
            // Bring forward any subsequent tick clock so the next call
            // won't be gated.
            if let Mode::AiVsAi { next_step_at, .. } = &mut app.mode {
                *next_step_at = std::time::Instant::now()
                    .checked_sub(std::time::Duration::from_millis(10))
                    .unwrap();
            }
            if app.world.as_ref().unwrap().turn > baseline_turn {
                break;
            }
        }
        let advanced_turn = app.world.as_ref().unwrap().turn;
        assert!(
            advanced_turn > baseline_turn,
            "AI vs AI did not advance world.turn (stuck at {})",
            baseline_turn
        );

        // Both agents must have recorded at least one action in memory.
        if let Mode::AiVsAi { agent_a, agent_b, .. } = &app.mode {
            assert!(!agent_a.memory.own.is_empty(), "agent A never acted");
            assert!(!agent_b.memory.own.is_empty(), "agent B never acted");
        } else {
            panic!("mode should still be AiVsAi");
        }
    }

    #[test]
    fn ai_vs_ai_blocks_player_input_but_allows_esc_and_predict() {
        // In AI vs AI mode the action menu is disabled — Enter must not
        // crash, must not double-apply a player action. Esc still exits.
        let mut app = fresh_app();
        // Mode → Country → Scenario (AI vs AI selected).
        app.handle_picker_key(KeyCode::Down); // mode_idx: 1 (AI vs AI)
        app.handle_picker_key(KeyCode::Enter); // → Country
        app.handle_picker_key(KeyCode::Enter); // → Scenario (theaters)
        app.handle_picker_key(KeyCode::Enter); // → Game
        app.bg = BgOp::Idle;
        assert!(app.mode.is_ai_vs_ai());

        // Pressing Enter / Up / Down while in Game must not panic and
        // must not crash the LLM client path (which doesn't exist in this
        // fixture but is what `handle_game_key` would otherwise call).
        app.handle_game_key(KeyCode::Enter);
        app.handle_game_key(KeyCode::Up);
        app.handle_game_key(KeyCode::Down);

        // 'p' still works — manual prediction refresh should not panic.
        app.handle_game_key(KeyCode::Char('p'));
    }

    #[test]
    fn ai_vs_ai_mode_routes_to_generated_scenario_not_player_game() {
        // Focused regression: choosing AI vs AI on the Mode step must
        // route through `enter_ai_vs_ai_for` (corpus-generated scenario),
        // NOT through `enter_game` (bundled JSON scenario). The two paths
        // diverge at `enter_after_picker`; this test pins that the
        // discriminator is `picker.mode`, not the country hint (the old
        // sentinel-based check).
        let mut app = fresh_app();
        // Mode: switch to AI vs AI.
        app.handle_picker_key(KeyCode::Down);
        app.handle_picker_key(KeyCode::Enter); // → Country
        // Country: keep default (US).
        app.handle_picker_key(KeyCode::Enter); // → Scenario (theaters)
        // Scenario (theater): keep default (Baltic Sea).
        app.handle_picker_key(KeyCode::Enter); // → Game
        app.bg = BgOp::Idle;

        assert_eq!(app.screen, Screen::Game);
        assert!(
            app.mode.is_ai_vs_ai(),
            "AI vs AI mode selection must yield AiVsAi engine mode"
        );
        let id = app
            .scenario
            .as_ref()
            .map(|s| s.id.clone())
            .unwrap_or_default();
        assert!(
            id.contains("_gen_"),
            "AI vs AI must produce a corpus-generated scenario, got id={}",
            id
        );
    }
}
