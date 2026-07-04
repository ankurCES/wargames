//! Top-level app state machine + main event loop.

use crate::config::BlumiSettings;
use crate::llm::{LlmClient, SOVIET_SYSTEM_PROMPT};
use crate::panes::game_layout;
use crate::picker::{
    default_countries, render_picker, Country, Picker, PickerStep, ScenarioEntry,
};
use crate::splash::render_splash;
use crate::tts::Tts;
use crate::widget_action::{render as render_action, ALL_ACTIONS};
use crate::widget_log::render as render_log;
use crate::widget_predict::render as render_predict;
use crate::widget_radar::render as render_radar;
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
}

impl App {
    pub fn new(settings: BlumiSettings, scenarios_dir: PathBuf) -> Self {
        let llm = LlmClient::from_settings(&settings);
        let tts = Tts::from_settings(&settings);
        let mut action_list = ListState::default();
        action_list.select(Some(0));
        let scenarios = load_scenarios(&scenarios_dir);
        let picker = Picker::new(default_countries(), scenarios);
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
            status: "ready".to_string(),
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
                false
            }
            KeyCode::Down => {
                self.picker.next();
                false
            }
            KeyCode::Enter => {
                if self.picker.advance() {
                    self.enter_game();
                }
                false
            }
            _ => false,
        }
    }

    pub fn handle_game_key(&mut self, code: KeyCode) -> bool {
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
        // Recompute prediction in foreground (cheap; deterministic; no I/O).
        let w = self.world.as_ref().unwrap().clone();
        let p = predict(&w, w.turn as u64 + 1, 200, 5);
        self.last_prediction = Some(p);
        self.last_prediction_at = Some(Instant::now());
        if wargames_core::engine::is_terminal(&w) {
            self.screen = Screen::GameOver;
            self.game_over_message = wargames_core::engine::game_over(&w).map(|o| match o {
                wargames_core::GameOutcome::Strike { by, .. } => format!("STRIKE by {:?}", by),
                wargames_core::GameOutcome::Disarm { by, .. } => format!("DISARM by {:?}", by),
                wargames_core::GameOutcome::Defect { by, .. } => format!("DEFECT by {:?}", by),
            });
        }
        self.status = format!("turn {} — you: {}", w.turn, action.as_str());
    }

    /// Best-effort opponent turn: if we have an LLM, queue a decision; for
    /// the TUI we don't actually run the LLM in the render loop (that would
    /// block), so we just synthesise a heuristic action for now.
    pub fn opponent_turn(&mut self) {
        let Some(world) = self.world.as_ref() else {
            return;
        };
        let opp_action = opponent_heuristic(world);
        let next = apply_action(world, Side::Opp, opp_action);
        self.world = Some(next);
        let w = self.world.as_ref().unwrap().clone();
        if wargames_core::engine::is_terminal(&w) {
            self.screen = Screen::GameOver;
            self.game_over_message = wargames_core::engine::game_over(&w).map(|o| match o {
                wargames_core::GameOutcome::Strike { by, .. } => format!("STRIKE by {:?}", by),
                wargames_core::GameOutcome::Disarm { by, .. } => format!("DISARM by {:?}", by),
                wargames_core::GameOutcome::Defect { by, .. } => format!("DEFECT by {:?}", by),
            });
        }
        let _ = self.llm.as_ref(); // touch — wired for a future async call
    }

    pub fn render(&mut self, frame: &mut ratatui::Frame) {
        match self.screen {
            Screen::Splash => {
                let secs_left = (5u8).saturating_sub(
                    self.splash_started_at.elapsed().as_secs().min(5) as u8,
                );
                render_splash(frame, frame.area(), secs_left);
            }
            Screen::Picker => {
                render_picker(frame, frame.area(), &mut self.picker);
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
            }
            Screen::GameOver => {
                self.render_game_over(frame);
            }
        }
    }

    fn render_status_line(&self, frame: &mut ratatui::Frame) {
        let area = ratatui::layout::Rect {
            x: 0,
            y: frame.area().height.saturating_sub(1),
            width: frame.area().width,
            height: 1,
        };
        let line = format!(
            " {}    [Tab] action  [Enter] commit  [Esc] quit",
            self.status
        );
        let p = ratatui::widgets::Paragraph::new(line).style(
            ratatui::style::Style::default().bg(ratatui::style::Color::Rgb(20, 20, 20)),
        );
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
        frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
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

#[allow(dead_code)]
fn _unused_marker(_: &PickerStep, _: &Country, _: &LlmClient) {}