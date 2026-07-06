//! Country + scenario + mode picker.
//!
//! Three-step flow:
//!   1. **Mode** — `Human vs AI` (uses bundled JSON scenarios) or
//!      `AI vs AI` (uses a corpus-generated scenario seeded by the
//!      theater the player picks).
//!   2. **Country** — faction the player will command.
//!   3. **Scenario** — for Human vs AI: a bundled `ScenarioEntry` from
//!      `scenarios/*.json`. For AI vs AI: a theater, which deterministically
//!      drives the seed used by the corpus generator.
//!
//! Each step requires an explicit `Enter` before the picker advances. There
//! is no auto-pick anywhere in the flow.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::Frame;
use wargames_core::Faction;
use wargames_core::Theater;
use crate::text;

/// The three picker steps. Order matters: Mode is the front door, Scenario
/// is the back door (immediately followed by `enter_after_picker`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PickerStep {
    Mode,
    Country,
    Scenario,
}

/// What the player chose on the Mode step. Persists onto `Picker.mode` so
/// `enter_after_picker` can route to the right game-entry function.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ModeChoice {
    HumanVsAi,
    AiVsAi,
}

/// One row on the Mode step.
#[derive(Debug, Clone)]
pub struct ModeEntry {
    pub choice: ModeChoice,
    pub title: String,
    pub description: String,
}

/// One row on the Scenario step **when in AI vs AI mode**. The seed is the
/// stable per-theater seed used by `corpus::generator::generate_scenario` —
/// same theater always generates the same opening read, but it's distinct
/// across theaters so the user gets a real choice.
#[derive(Debug, Clone)]
pub struct TheaterEntry {
    pub theater: Theater,
    pub seed: u64,
}

#[derive(Debug, Clone)]
pub struct Picker {
    pub step: PickerStep,
    pub mode: ModeChoice,
    pub mode_idx: usize,
    pub country_idx: usize,
    pub scenario_idx: usize,
    pub modes: Vec<ModeEntry>,
    pub countries: Vec<Country>,
    pub scenarios: Vec<ScenarioEntry>,
    pub theaters: Vec<TheaterEntry>,
    pub list_state: ListState,
    /// Cached filtered scenarios — invalidated by `next`/`prev`/`advance`.
    /// For Human vs AI this is the bundled JSON IDs filtered by faction.
    /// For AI vs AI this is the chosen theater id (always non-empty —
    /// every theater is selectable). Stored as owned ids so we can return
    /// references without unsafe.
    filtered_cache: Vec<String>,
    /// Render-time message shown when the filtered scenario list is empty
    /// for the player's faction. `None` when the list is non-empty.
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Country {
    pub faction: Faction,
    pub hint: String,
}

#[derive(Debug, Clone)]
pub struct ScenarioEntry {
    pub id: String,
    pub title: String,
    pub defcon: u8,
    pub theater: String,
    pub faction: Faction,
}

impl Picker {
    pub fn new(
        modes: Vec<ModeEntry>,
        countries: Vec<Country>,
        scenarios: Vec<ScenarioEntry>,
        theaters: Vec<TheaterEntry>,
    ) -> Self {
        let mut s = ListState::default();
        s.select(Some(0));
        // NOTE: do NOT call rebuild_cache() here. The cache is meaningful only
        // in Scenario step; pre-computing it on the Country step makes the
        // renderer surface a phantom "no scenarios match this faction" error
        // before the player has even pressed Enter. The cache is built
        // lazily inside `advance()` (Country → Scenario) and inside `next`/
        // `prev` when the player rotates the country highlight.
        Self {
            step: PickerStep::Mode,
            mode: ModeChoice::HumanVsAi,
            mode_idx: 0,
            country_idx: 0,
            scenario_idx: 0,
            modes,
            countries,
            scenarios,
            theaters,
            list_state: s,
            filtered_cache: Vec::new(),
            error: None,
        }
    }

    fn rebuild_cache(&mut self) {
        // The cache only makes sense after the player has stepped past the
        // country list. On the Mode or Country step, leave both fields
        // empty so the renderer never paints an empty-state message that
        // doesn't belong to the current step.
        if self.step != PickerStep::Scenario {
            self.filtered_cache.clear();
            self.error = None;
            return;
        }
        // AI vs AI: the Scenario step is the theater picker. The "filtered
        // cache" holds the selected theater id so `selected_scenario()`
        // can return a stable value. No faction filter — every theater is
        // playable regardless of country choice.
        if self.mode == ModeChoice::AiVsAi {
            self.filtered_cache = self
                .theaters
                .iter()
                .map(|t| format!("{:?}", t.theater))
                .collect();
            self.error = None;
            if self.scenario_idx >= self.filtered_cache.len() {
                self.scenario_idx = self.filtered_cache.len().saturating_sub(1);
            }
            return;
        }
        // Human vs AI: filter bundled JSON scenarios by the selected
        // faction's visibility matrix.
        let faction = self.countries.get(self.country_idx).map(|c| c.faction);
        // A faction "plays" any scenario tagged for itself OR for any
        // great-power bloc in the same theater of operations. Without this
        // widening, picking PRC or DPRK (which have no JSON-tagged scenarios
        // yet) leaves the player with an empty list and no signal that the
        // picker is hung.
        //
        // Visibility matrix:
        //   Us     → Us | Nato | Soviet        (Cold-War great-power tag set)
        //   Nato   → Nato | Us                 (alliance partner)
        //   Soviet → Soviet | Nato | Us        (mirror of Us)
        //   Prc    → Prc | Us | Nato | Soviet  (modern peer; sees everything)
        //   Dprk   → Us | Nato                 (Korean theater; no Soviet)
        let accepted = |sf: Faction| -> bool {
            match faction {
                Some(Faction::Us) => {
                    matches!(sf, Faction::Us | Faction::Nato | Faction::Soviet)
                }
                Some(Faction::Nato) => matches!(sf, Faction::Nato | Faction::Us),
                Some(Faction::Soviet) => {
                    matches!(sf, Faction::Soviet | Faction::Nato | Faction::Us)
                }
                Some(Faction::Prc) => matches!(
                    sf,
                    Faction::Prc | Faction::Us | Faction::Nato | Faction::Soviet
                ),
                Some(Faction::Dprk) => matches!(sf, Faction::Us | Faction::Nato),
                None => true,
            }
        };
        self.filtered_cache = self
            .scenarios
            .iter()
            .filter(|s| accepted(s.faction))
            .map(|s| s.id.clone())
            .collect();
        // Surface an explicit empty-state message rather than a silent blank.
        self.error = if self.filtered_cache.is_empty() {
            Some("no scenarios match this faction — press Esc to go back".to_string())
        } else {
            None
        };
        // Clamp scenario_idx.
        if self.scenario_idx >= self.filtered_cache.len() {
            self.scenario_idx = self.filtered_cache.len().saturating_sub(1);
        }
    }

    fn current_list_len(&self) -> usize {
        match self.step {
            PickerStep::Mode => self.modes.len(),
            PickerStep::Country => self.countries.len(),
            PickerStep::Scenario => self.filtered_cache.len(),
        }
    }

    pub fn next(&mut self) {
        let len = self.current_list_len();
        if len == 0 {
            return;
        }
        let i = self.list_state.selected().unwrap_or(0);
        let next = (i + 1) % len;
        self.list_state.select(Some(next));
        match self.step {
            PickerStep::Mode => self.mode_idx = next,
            PickerStep::Country => self.country_idx = next,
            PickerStep::Scenario => self.scenario_idx = next,
        }
    }

    pub fn prev(&mut self) {
        let len = self.current_list_len();
        if len == 0 {
            return;
        }
        let i = self.list_state.selected().unwrap_or(0);
        let prev = if i == 0 { len - 1 } else { i - 1 };
        self.list_state.select(Some(prev));
        match self.step {
            PickerStep::Mode => self.mode_idx = prev,
            PickerStep::Country => self.country_idx = prev,
            PickerStep::Scenario => self.scenario_idx = prev,
        }
    }

    /// Advance one step. Returns `true` when the picker has reached the
    /// end (Scenario confirmed) and the caller should enter the game.
    pub fn advance(&mut self) -> bool {
        match self.step {
            PickerStep::Mode => {
                // Latch the mode choice now so the Country/Scenario steps
                // know which list to show. Default-Index 0 → HumanVsAi.
                self.mode = self
                    .modes
                    .get(self.mode_idx)
                    .map(|m| m.choice)
                    .unwrap_or(ModeChoice::HumanVsAi);
                self.step = PickerStep::Country;
                self.list_state.select(Some(self.country_idx));
                self.rebuild_cache();
                false
            }
            PickerStep::Country => {
                self.step = PickerStep::Scenario;
                self.list_state.select(Some(0));
                self.scenario_idx = 0;
                self.rebuild_cache();
                false
            }
            PickerStep::Scenario => match self.mode {
                ModeChoice::HumanVsAi => !self.filtered_cache.is_empty(),
                // AI vs AI: every theater is playable, so the list is
                // always non-empty by construction.
                ModeChoice::AiVsAi => true,
            },
        }
    }

    /// Step backwards. Returns `true` only when the player has left the
    /// picker entirely (back from the Mode step) — the App layer treats
    /// that as quit. Stepping back from Scenario or Country stays in the
    /// picker and returns `false`.
    pub fn back(&mut self) -> bool {
        match self.step {
            PickerStep::Scenario => {
                self.step = PickerStep::Country;
                self.list_state.select(Some(self.country_idx));
                false
            }
            PickerStep::Country => {
                self.step = PickerStep::Mode;
                self.list_state.select(Some(self.mode_idx));
                self.rebuild_cache();
                false
            }
            PickerStep::Mode => true, // signal quit
        }
    }

    pub fn selected_mode(&self) -> Option<&ModeEntry> {
        self.modes.get(self.mode_idx)
    }

    pub fn selected_country(&self) -> Option<&Country> {
        self.countries.get(self.country_idx)
    }

    /// Returns the currently-highlighted scenario, if any.
    ///
    /// - Human vs AI: the bundled `ScenarioEntry` whose id is in the
    ///   filtered cache.
    /// - AI vs AI: the `TheaterEntry` selected on the Scenario step. The
    ///   app layer feeds its `(theater, seed)` into
    ///   `enter_ai_vs_ai_with_seed`.
    pub fn selected_scenario(&self) -> Option<SelectedScenario<'_>> {
        let id = self.filtered_cache.get(self.scenario_idx)?;
        match self.mode {
            ModeChoice::HumanVsAi => self
                .scenarios
                .iter()
                .find(|s| &s.id == id)
                .map(SelectedScenario::Bundled),
            ModeChoice::AiVsAi => {
                // The cache holds format!("{:?}", theater); reverse-lookup.
                self.theaters
                    .iter()
                    .find(|t| format!("{:?}", t.theater) == *id)
                    .map(SelectedScenario::Theater)
            }
        }
    }

    /// What the picker is showing on the Scenario step right now. AI vs AI
    /// returns the theater list directly (so the renderer can draw them
    /// without going through `ScenarioEntry`).
    pub fn scenario_list(&self) -> ScenarioList<'_> {
        match self.mode {
            ModeChoice::HumanVsAi => {
                ScenarioList::Bundled(self.filtered_scenarios())
            }
            ModeChoice::AiVsAi => ScenarioList::Theaters(self.theaters_for_display()),
        }
    }

    pub fn filtered_scenarios(&self) -> Vec<&ScenarioEntry> {
        self.filtered_cache
            .iter()
            .filter_map(|id| self.scenarios.iter().find(|s| &s.id == id))
            .collect()
    }

    fn theaters_for_display(&self) -> Vec<&TheaterEntry> {
        // For AI vs AI we display every theater — there's no faction
        // filter on the Scenario step in this mode.
        self.theaters.iter().collect()
    }
}

/// Discriminated union returned by `Picker::selected_scenario()`. The App
/// layer pattern-matches on this to call the right entry function.
#[derive(Debug, Clone)]
pub enum SelectedScenario<'a> {
    Bundled(&'a ScenarioEntry),
    Theater(&'a TheaterEntry),
}

/// What the picker renders on the Scenario step. Used by `render_picker`
/// to decide whether to draw the JSON-scenario rows or the theater rows.
#[derive(Debug, Clone)]
pub enum ScenarioList<'a> {
    Bundled(Vec<&'a ScenarioEntry>),
    Theaters(Vec<&'a TheaterEntry>),
}

pub fn render_picker(frame: &mut Frame, area: Rect, picker: &mut Picker) {
    frame.render_widget(Clear, area);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // bordered title strip
            Constraint::Min(1),    // list (or empty-state message)
            Constraint::Length(1), // status bar
        ])
        .split(area);

    let title = match picker.step {
        PickerStep::Mode => "PICK A MODE",
        PickerStep::Country => "PICK A COUNTRY",
        PickerStep::Scenario => match picker.mode {
            ModeChoice::HumanVsAi => "PICK A SCENARIO",
            ModeChoice::AiVsAi => "PICK A THEATER",
        },
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Green))
        .title(Span::styled(
            format!(" {title} "),
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ));
    frame.render_widget(block, chunks[0]);

    // Empty-state branch — explicit, no silent blank list. Only the
    // Scenario step can ever produce this (Human vs AI + a faction with
    // no matching scenarios).
    if let Some(msg) = picker.error.as_ref() {
        let p = Paragraph::new(Line::from(Span::styled(
            format!("  {msg}"),
            Style::default().fg(Color::LightRed).add_modifier(Modifier::ITALIC),
        )))
        .wrap(Wrap { trim: false });
        frame.render_widget(p, chunks[1]);
        render_picker_status(frame, chunks[2], picker, "");
        return;
    }

    // The list area is chunks[1]. Compute its inner width once so we
    // can wrap descriptions that would otherwise overflow the row on
    // narrow terminals (we never drop text — wrap, don't truncate).
    let inner_w = chunks[1].width.saturating_sub(4) as usize; // block padding

    let items: Vec<ListItem> = match picker.step {
        PickerStep::Mode => picker
            .modes
            .iter()
            .map(|m| {
                // Reserve at least 14 cells for the mode title so the
                // bold-tag column stays aligned across list items; on
                // very narrow panes the title column shrinks but still
                // shows the value.
                let title_col = inner_w.min(14);
                let body_col = inner_w.saturating_sub(title_col + 3);
                let title = format!("  {:<title_w$}", m.title, title_w = title_col);
                // Wrap, never truncate — body text stays readable on
                // every screen size we support.
                let body_lines = text::wrap_to_width(&m.description, body_col.max(8));
                let mut spans: Vec<Line> = Vec::with_capacity(1 + body_lines.len());
                spans.push(Line::from(vec![
                    Span::styled(
                        title,
                        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        body_lines.first().cloned().unwrap_or_default(),
                        Style::default().fg(Color::Gray),
                    ),
                ]));
                for extra in body_lines.iter().skip(1) {
                    // Indent continuations so they line up under the body.
                    let indent = " ".repeat(title_col + 2);
                    spans.push(Line::from(Span::styled(
                        format!("{indent}{extra}"),
                        Style::default().fg(Color::DarkGray),
                    )));
                }
                ListItem::new(spans)
            })
            .collect(),
        PickerStep::Country => picker
            .countries
            .iter()
            .map(|c| {
                let title = format!("  {} ", c.faction.display_name());
                let title_col = text::display_width(&title);
                let body_col = inner_w.saturating_sub(title_col + 1);
                let body_lines = text::wrap_to_width(&c.hint, body_col.max(8));
                let mut spans: Vec<Line> = Vec::with_capacity(1 + body_lines.len());
                spans.push(Line::from(vec![
                    Span::styled(
                        title,
                        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        body_lines.first().cloned().unwrap_or_default(),
                        Style::default().fg(Color::Gray),
                    ),
                ]));
                for extra in body_lines.iter().skip(1) {
                    let indent = " ".repeat(title_col);
                    spans.push(Line::from(Span::styled(
                        format!("{indent}{extra}"),
                        Style::default().fg(Color::DarkGray),
                    )));
                }
                ListItem::new(spans)
            })
            .collect(),
        PickerStep::Scenario => match picker.scenario_list() {
            ScenarioList::Bundled(entries) => entries
                .into_iter()
                .map(|s| {
                    ListItem::new(Line::from(vec![
                        Span::styled(
                            format!("  DEFCON {}  ", s.defcon),
                            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(
                            format!("{:<22}", s.title),
                            Style::default().fg(Color::White),
                        ),
                        Span::styled(
                            format!("[{}]", s.theater),
                            Style::default().fg(Color::DarkGray),
                        ),
                    ]))
                })
                .collect(),
            ScenarioList::Theaters(entries) => entries
                .into_iter()
                .map(|t| {
                    ListItem::new(Line::from(vec![
                        Span::styled(
                            format!("  {:<22}", t.theater.display_name()),
                            Style::default().fg(Color::White),
                        ),
                        Span::styled(
                            format!("[seed 0x{:08x}]", t.seed),
                            Style::default().fg(Color::DarkGray),
                        ),
                    ]))
                })
                .collect(),
        },
    };

    let list = List::new(items)
        .highlight_style(
            Style::default()
                .bg(Color::Rgb(52, 0, 0))
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("> ");
    frame.render_stateful_widget(list, chunks[1], &mut picker.list_state);
    render_picker_status(frame, chunks[2], picker, "");
}

/// Renders the 1-line status bar at the bottom of the picker pane. The
/// caller passes an optional overlay string (e.g. "LOADING…") that takes
/// visual precedence when the run loop is busy with scenario work.
pub fn render_picker_status(frame: &mut Frame, area: Rect, picker: &Picker, overlay: &str) {
    let body = if !overlay.is_empty() {
        format!(" » {overlay}")
    } else {
        match picker.step {
            PickerStep::Mode => format!(
                " pick a mode (↑↓ select, Enter confirm) — {} available",
                picker.modes.len()
            ),
            PickerStep::Country => format!(
                " pick a country (↑↓ select, Enter confirm, Esc back) — {} available",
                picker.countries.len()
            ),
            PickerStep::Scenario => match picker.mode {
                ModeChoice::HumanVsAi => format!(
                    " pick a scenario (↑↓ select, Enter confirm, Esc back) — {} filtered",
                    picker.filtered_cache.len()
                ),
                ModeChoice::AiVsAi => format!(
                    " pick a theater for AI vs AI (↑↓ select, Enter confirm, Esc back) — {} available",
                    picker.theaters.len()
                ),
            },
        }
    };
    let p = Paragraph::new(body).style(Style::default().bg(Color::Rgb(20, 20, 20)));
    frame.render_widget(p, area);
}

/// The two modes shown on the front step. Order is the order they appear
/// in the picker list — Human vs AI first, AI vs AI second.
pub fn default_modes() -> Vec<ModeEntry> {
    vec![
        ModeEntry {
            choice: ModeChoice::HumanVsAi,
            title: "Human vs AI".to_string(),
            description: "you command one side; the AI runs the other".to_string(),
        },
        ModeEntry {
            choice: ModeChoice::AiVsAi,
            title: "AI vs AI".to_string(),
            description: "two learned agents play a self-ticking match".to_string(),
        },
    ]
}

pub fn default_countries() -> Vec<Country> {
    vec![
        Country {
            faction: Faction::Us,
            hint: "Carrier groups, B-2s, NATO alliance".to_string(),
        },
        Country {
            faction: Faction::Soviet,
            hint: "Submarines, ICBMs, Spetsnaz".to_string(),
        },
        Country {
            faction: Faction::Nato,
            hint: "Collective defense, Article 5".to_string(),
        },
        Country {
            faction: Faction::Prc,
            hint: "Carrier program, ADIZ doctrine".to_string(),
        },
        Country {
            faction: Faction::Dprk,
            hint: "Limited arsenal, high noise".to_string(),
        },
    ]
}

/// The 8 theaters the AI vs AI Scenario step offers. The seed is the
/// stable per-theater seed the corpus generator uses — same theater →
/// same opening read on every run.
pub fn default_theaters() -> Vec<TheaterEntry> {
    let theaters = [
        Theater::BalticSea,
        Theater::BlackSea,
        Theater::KoreanPeninsula,
        Theater::TaiwanStrait,
        Theater::SouthChinaSea,
        Theater::RedSea,
        Theater::EasternMed,
        Theater::NorthAtlantic,
    ];
    theaters
        .into_iter()
        .enumerate()
        .map(|(i, theater)| TheaterEntry {
            theater,
            // Stable per-index seed. Different theaters → different
            // openings; same theater → identical every run (replayable).
            seed: 0xA1A1_0001 + i as u64,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use wargames_core::Faction;

    fn entry(id: &str, faction: Faction) -> ScenarioEntry {
        ScenarioEntry {
            id: id.into(),
            title: id.into(),
            defcon: 3,
            theater: "test".into(),
            faction,
        }
    }

    fn fixture() -> Picker {
        Picker::new(
            default_modes(),
            default_countries(),
            vec![
                entry("us_a", Faction::Us),
                entry("nato_a", Faction::Nato),
                entry("soviet_a", Faction::Soviet),
                entry("prc_a", Faction::Prc),
                entry("dprk_a", Faction::Dprk),
            ],
            default_theaters(),
        )
    }

    // ---------- Mode step ----------

    #[test]
    fn fresh_picker_starts_on_mode_step_with_human_vs_ai_default() {
        let p = fixture();
        assert_eq!(p.step, PickerStep::Mode, "fresh picker must start on Mode");
        assert_eq!(
            p.mode,
            ModeChoice::HumanVsAi,
            "default mode before advance() is HumanVsAi"
        );
        assert_eq!(p.mode_idx, 0);
        assert!(p.error.is_none());
        assert!(p.filtered_cache.is_empty());
    }

    #[test]
    fn mode_step_lists_exactly_two_modes() {
        let modes = default_modes();
        assert_eq!(modes.len(), 2);
        assert_eq!(modes[0].choice, ModeChoice::HumanVsAi);
        assert_eq!(modes[1].choice, ModeChoice::AiVsAi);
    }

    #[test]
    fn advance_from_mode_to_country_latches_mode_choice() {
        let mut p = fixture();
        // Move to AI vs AI on the Mode step.
        p.next();
        assert_eq!(p.mode_idx, 1);
        let done = p.advance();
        assert!(!done, "Mode→Country is not the entry-into-game transition");
        assert_eq!(p.step, PickerStep::Country);
        assert_eq!(
            p.mode,
            ModeChoice::AiVsAi,
            "the mode choice must be latched before Scenario is shown"
        );
    }

    // ---------- Country step (unchanged behavior) ----------

    #[test]
    fn usa_sees_us_and_nato_scenarios() {
        let mut p = fixture();
        // Mode → Country → Scenario
        p.advance();
        p.advance();
        let ids: Vec<&str> = match p.scenario_list() {
            ScenarioList::Bundled(entries) => {
                entries.iter().map(|s| s.id.as_str()).collect()
            }
            ScenarioList::Theaters(_) => panic!("Human vs AI must list bundled scenarios"),
        };
        assert_eq!(ids, vec!["us_a", "nato_a", "soviet_a"]);
    }

    #[test]
    fn soviet_sees_us_nato_and_soviet_scenarios() {
        // Need a Soviet-tagging country. Use a fresh picker with explicit
        // country list since default_countries() has Us first.
        let mut p = Picker::new(
            default_modes(),
            vec![Country { faction: Faction::Soviet, hint: "".into() }],
            vec![
                entry("us_a", Faction::Us),
                entry("nato_a", Faction::Nato),
                entry("soviet_a", Faction::Soviet),
            ],
            default_theaters(),
        );
        p.advance(); // Mode → Country
        p.advance(); // Country → Scenario
        let ids: Vec<&str> = match p.scenario_list() {
            ScenarioList::Bundled(entries) => {
                entries.iter().map(|s| s.id.as_str()).collect()
            }
            ScenarioList::Theaters(_) => panic!(),
        };
        assert_eq!(ids, vec!["us_a", "nato_a", "soviet_a"]);
    }

    #[test]
    fn advance_resets_scenario_index() {
        let mut p = fixture();
        p.advance(); // Mode → Country
        p.advance(); // Country → Scenario
        assert_eq!(p.scenario_idx, 0);
        assert_eq!(p.list_state.selected(), Some(0));
    }

    #[test]
    fn prc_sees_modern_great_power_set() {
        let mut p = Picker::new(
            default_modes(),
            vec![Country { faction: Faction::Prc, hint: "".into() }],
            vec![
                entry("us_a", Faction::Us),
                entry("nato_a", Faction::Nato),
                entry("soviet_a", Faction::Soviet),
                entry("prc_a", Faction::Prc),
            ],
            default_theaters(),
        );
        p.advance();
        p.advance();
        let ids: Vec<&str> = match p.scenario_list() {
            ScenarioList::Bundled(entries) => {
                entries.iter().map(|s| s.id.as_str()).collect()
            }
            ScenarioList::Theaters(_) => panic!(),
        };
        assert_eq!(ids, vec!["us_a", "nato_a", "soviet_a", "prc_a"]);
    }

    #[test]
    fn dprk_sees_us_nato_only() {
        let mut p = Picker::new(
            default_modes(),
            vec![Country { faction: Faction::Dprk, hint: "".into() }],
            vec![
                entry("us_a", Faction::Us),
                entry("nato_a", Faction::Nato),
                entry("soviet_a", Faction::Soviet),
                entry("prc_a", Faction::Prc),
            ],
            default_theaters(),
        );
        p.advance();
        p.advance();
        let ids: Vec<&str> = match p.scenario_list() {
            ScenarioList::Bundled(entries) => {
                entries.iter().map(|s| s.id.as_str()).collect()
            }
            ScenarioList::Theaters(_) => panic!(),
        };
        assert_eq!(ids, vec!["us_a", "nato_a"]);
    }

    #[test]
    fn empty_state_sets_error_message() {
        let mut p = Picker::new(
            default_modes(),
            vec![Country { faction: Faction::Dprk, hint: "".into() }],
            vec![entry("x", Faction::Prc), entry("y", Faction::Soviet)],
            default_theaters(),
        );
        p.advance();
        p.advance();
        let bundled = match p.scenario_list() {
            ScenarioList::Bundled(entries) => entries,
            ScenarioList::Theaters(_) => panic!("DPRK + Human vs AI should show bundled scenarios"),
        };
        assert!(bundled.is_empty());
        assert!(p.error.is_some(), "must surface an empty-state message");
        assert!(p.advance() == false, "Enter on empty list must not enter the game");
    }

    // ---------- AI vs AI Scenario step ----------

    #[test]
    fn ai_vs_ai_scenario_step_lists_theaters_not_bundled_json() {
        let mut p = fixture();
        p.advance(); // Mode → Country (default mode: HumanVsAi)
        p.next(); // toggle to AI vs AI on the Mode step? No — Mode already
                  // advanced. The mode was latched when advance() ran.
                  // So this scenario isn't right. Let me redo.
        // Actually: this test wants to verify the AI-vs-AI Scenario step
        // shows theaters. We need to switch the mode *before* the
        // Country→Scenario transition.
        let mut p = fixture();
        p.next(); // mode_idx: 0 → 1 (AI vs AI)
        p.advance(); // Mode → Country, latches AiVsAi
        p.advance(); // Country → Scenario
        match p.scenario_list() {
            ScenarioList::Theaters(entries) => {
                assert_eq!(entries.len(), 8);
                assert!(entries.iter().any(|t| t.theater == Theater::BalticSea));
                assert!(entries.iter().any(|t| t.theater == Theater::TaiwanStrait));
            }
            ScenarioList::Bundled(_) => {
                panic!("AI vs AI Scenario step must list theaters, not bundled JSON")
            }
        }
    }

    #[test]
    fn ai_vs_ai_advance_from_scenario_returns_true_every_time() {
        // AI vs AI: every theater is playable, so advance() at Scenario
        // must always succeed (no empty-state path).
        let mut p = fixture();
        p.next(); // mode_idx: 1 (AiVsAi)
        p.advance(); // → Country
        p.advance(); // → Scenario
        assert!(p.advance(), "AI vs AI: advance at Scenario must succeed");
    }

    #[test]
    fn ai_vs_ai_selected_scenario_returns_theater_with_seed() {
        let mut p = fixture();
        p.next();
        p.advance();
        p.advance();
        // Pick the third theater.
        p.next();
        p.next();
        let sel = p.selected_scenario().expect("must select a theater");
        match sel {
            SelectedScenario::Theater(t) => {
                // Stable per-theater seed.
                assert_eq!(t.seed, 0xA1A1_0001 + 2);
                assert_eq!(t.theater, Theater::KoreanPeninsula);
            }
            SelectedScenario::Bundled(_) => panic!("AI vs AI must return a Theater"),
        }
    }

    #[test]
    fn human_vs_ai_selected_scenario_returns_bundled_entry() {
        let mut p = fixture();
        // Default mode is HumanVsAi; just advance through.
        p.advance(); // → Country
        p.advance(); // → Scenario
        let sel = p.selected_scenario().expect("must select a scenario");
        match sel {
            SelectedScenario::Bundled(s) => {
                // US sees US + NATO + Soviet; default idx is 0 → us_a.
                assert_eq!(s.id, "us_a");
                assert_eq!(s.faction, Faction::Us);
            }
            SelectedScenario::Theater(_) => panic!("Human vs AI must return a Bundled entry"),
        }
    }

    // ---------- Backwards navigation ----------

    #[test]
    fn back_walks_through_all_three_steps() {
        let mut p = fixture();
        assert_eq!(p.step, PickerStep::Mode);
        p.advance();
        assert_eq!(p.step, PickerStep::Country);
        p.advance();
        assert_eq!(p.step, PickerStep::Scenario);
        // Scenario → Country
        assert!(!p.back(), "back from Scenario stays in picker");
        assert_eq!(p.step, PickerStep::Country);
        // Country → Mode
        assert!(!p.back(), "back from Country stays in picker");
        assert_eq!(p.step, PickerStep::Mode);
        // Mode → quit
        assert!(p.back(), "back from Mode signals quit to the App layer");
    }

    #[test]
    fn back_from_country_preserves_mode_highlight() {
        let mut p = fixture();
        p.next(); // mode_idx: 1 (AiVsAi)
        p.advance(); // → Country
        assert_eq!(p.mode_idx, 1);
        p.back(); // → Mode
        assert_eq!(p.list_state.selected(), Some(1));
        assert_eq!(p.mode_idx, 1);
    }

    // ---------- Regression: fresh picker shows no error on any step ----------

    #[test]
    fn fresh_picker_on_mode_step_has_no_error_and_empty_cache() {
        let p = fixture();
        assert_eq!(p.step, PickerStep::Mode);
        assert!(p.error.is_none(), "fresh picker must not show an error");
        assert!(p.filtered_cache.is_empty());
    }

    #[test]
    fn fresh_picker_on_country_step_has_no_error_and_empty_cache() {
        let mut p = fixture();
        p.advance();
        assert_eq!(p.step, PickerStep::Country);
        assert!(p.error.is_none(), "Country step must not show an error");
        assert!(p.filtered_cache.is_empty());
    }

    #[test]
    fn stale_error_on_country_step_is_cleared_by_rebuild_cache() {
        let mut p = fixture();
        p.advance(); // → Country
        p.error = Some("stale".into());
        // Force a rebuild (happens on next/prev/advance).
        p.rebuild_cache();
        assert_eq!(p.step, PickerStep::Country);
        assert!(
            p.error.is_none(),
            "rebuild_cache must clear any stale error on the Country step"
        );
    }

    // ---------- Reachability for every faction (preserved regression net) ----------

    #[test]
    fn every_faction_advances_to_scenarios_with_results() {
        use std::collections::HashSet;
        let scenarios = vec![
            entry("us_a", Faction::Us),
            entry("nato_a", Faction::Nato),
            entry("soviet_a", Faction::Soviet),
            entry("prc_a", Faction::Prc),
            entry("dprk_a", Faction::Dprk),
        ];
        let cases = [
            (Faction::Us, &["us_a", "nato_a", "soviet_a"][..]),
            (Faction::Nato, &["us_a", "nato_a"][..]),
            (Faction::Soviet, &["us_a", "nato_a", "soviet_a"][..]),
            (Faction::Prc, &["us_a", "nato_a", "soviet_a", "prc_a"][..]),
            (Faction::Dprk, &["us_a", "nato_a"][..]),
        ];
        for (faction, expected_visible) in cases {
            let mut p = Picker::new(
                default_modes(),
                vec![Country { faction, hint: "".into() }],
                scenarios.clone(),
                default_theaters(),
            );
            // Mode → Country (transition #1, returns false)
            assert!(!p.advance());
            assert_eq!(p.step, PickerStep::Country);
            // Country → Scenario (transition #2, returns false)
            assert!(!p.advance());
            assert_eq!(p.step, PickerStep::Scenario);
            let ids: Vec<&str> = match p.scenario_list() {
                ScenarioList::Bundled(entries) => {
                    entries.iter().map(|s| s.id.as_str()).collect()
                }
                ScenarioList::Theaters(_) => panic!("{faction:?}: must list bundled"),
            };
            assert!(!ids.is_empty(), "{faction:?}: must see ≥1 scenario");
            let seen: HashSet<&str> = ids.into_iter().collect();
            let expected: HashSet<&str> = expected_visible.iter().copied().collect();
            assert_eq!(
                seen, expected,
                "{faction:?}: visibility matrix regression"
            );
            assert!(p.error.is_none(), "{faction:?}: must not surface an error");
            // Scenario → enter game (returns true when the list is non-empty)
            assert!(p.advance(), "{faction:?}: must enter game from Scenario");
        }
    }

    // ---------- Theaters list ----------

    #[test]
    fn default_theaters_returns_all_eight_supported_theaters() {
        let ts = default_theaters();
        assert_eq!(ts.len(), 8);
        // Each seed is unique (different theaters → different openings).
        let seeds: std::collections::HashSet<u64> = ts.iter().map(|t| t.seed).collect();
        assert_eq!(seeds.len(), 8, "theater seeds must be unique");
    }
}