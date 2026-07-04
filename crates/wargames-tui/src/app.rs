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
use wargames_core::{Action, Posture, Side, SideState};
use wargames_core::WorldState;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Screen {
    Splash,
    Picker,
    Game,
    GameOver,
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
}

impl App {
    pub fn new(settings: BlumiSettings, scenarios_dir: PathBuf) -> Self {
        let llm = LlmClient::from_settings(&settings);
        let tts = Tts::from_settings(&settings);
        let mut action_list = ListState::default();
        action_list.select(Some(0));
        let scenarios = load_scenarios(&scenarios_dir);
        let picker = Picker::new(default_countries(), scenarios);
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
        let Some(entry) = self.picker.selected_scenario().cloned() else {
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
        self.scenario_entry = Some(entry);
        self.screen = Screen::Game;
        self.status = format!("engaged — DEFCON {}", self.world.as_ref().unwrap().defcon);
    }

    pub fn handle_picker_key(&mut self, code: KeyCode) -> bool {
        match code {
            KeyCode::Esc => {
                if self.picker.back() {
                    false
                } else {
                    true
                }
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
                if done && !self.bg.is_busy() {
                    self.enter_game();
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
        if let Some(elapsed) = self.scenario_load_elapsed() {
            if elapsed >= Duration::from_millis(900) {
                self.bg = BgOp::Idle;
            }
        }
        // Advance the spinner when work is in flight so the user sees motion.
        if self.bg.is_busy() {
            self.spinner_frame = self.spinner_frame.wrapping_add(1);
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
        PickerStep::Country => format!(
            "pick a country (↑↓ select, Enter confirm) — {} available",
            picker.countries.len()
        ),
        PickerStep::Scenario => format!(
            "pick a scenario (↑↓ select, Enter confirm, Esc back) — {} filtered",
            picker.filtered_scenarios().len()
        ),
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
        // `BlumiSettings` doesn't derive `Default` (only `Deserialize`),
        // so build a minimal-but-valid settings JSON on disk and load it
        // through `BlumiSettings::from_path`. The resulting struct has no
        // providers, so `App::new` constructs no `LlmClient` and the
        // heuristic opponent is the one that runs in-game.
        let mut path = std::env::temp_dir();
        path.push(format!(
            "wargames_playable_{}_{}.json",
            std::process::id(),
            line!()
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
        assert_eq!(app.picker.step, PickerStep::Country);

        // 1) Enter on the country step → moves to scenario, shows spinner.
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

        // 2) Enter on the scenario step → game screen, world populated.
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
        // Drive to the game screen.
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
        app.handle_picker_key(KeyCode::Enter);
        assert!(
            matches!(app.bg, BgOp::ScenarioLoad { .. }),
            "spinner must trigger on country→scenario step"
        );
        assert_eq!(app.picker.step, PickerStep::Scenario);
    }
}
