//! Country + scenario picker.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState};
use ratatui::Frame;
use wargames_core::Faction;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PickerStep {
    Country,
    Scenario,
}

#[derive(Debug, Clone)]
pub struct Picker {
    pub step: PickerStep,
    pub country_idx: usize,
    pub scenario_idx: usize,
    pub countries: Vec<Country>,
    pub scenarios: Vec<ScenarioEntry>,
    pub list_state: ListState,
    /// Cached filtered scenarios — invalidated by `next`/`prev`/`advance`.
    /// Stored as owned ids so we can return references without unsafe.
    filtered_cache: Vec<String>,
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
    pub fn new(countries: Vec<Country>, scenarios: Vec<ScenarioEntry>) -> Self {
        let mut s = ListState::default();
        s.select(Some(0));
        let mut p = Self {
            step: PickerStep::Country,
            country_idx: 0,
            scenario_idx: 0,
            countries,
            scenarios,
            list_state: s,
            filtered_cache: Vec::new(),
        };
        p.rebuild_cache();
        p
    }

    fn rebuild_cache(&mut self) {
        let faction = self.countries.get(self.country_idx).map(|c| c.faction);
        self.filtered_cache = self
            .scenarios
            .iter()
            .filter(|s| match faction {
                Some(f) => s.faction == f,
                None => true,
            })
            .map(|s| s.id.clone())
            .collect();
        // Clamp scenario_idx.
        if self.scenario_idx >= self.filtered_cache.len() {
            self.scenario_idx = self.filtered_cache.len().saturating_sub(1);
        }
    }

    pub fn next(&mut self) {
        let len = match self.step {
            PickerStep::Country => self.countries.len(),
            PickerStep::Scenario => self.filtered_cache.len(),
        };
        if len == 0 {
            return;
        }
        let i = self.list_state.selected().unwrap_or(0);
        let next = (i + 1) % len;
        self.list_state.select(Some(next));
        match self.step {
            PickerStep::Country => self.country_idx = next,
            PickerStep::Scenario => self.scenario_idx = next,
        }
    }

    pub fn prev(&mut self) {
        let len = match self.step {
            PickerStep::Country => self.countries.len(),
            PickerStep::Scenario => self.filtered_cache.len(),
        };
        if len == 0 {
            return;
        }
        let i = self.list_state.selected().unwrap_or(0);
        let prev = if i == 0 { len - 1 } else { i - 1 };
        self.list_state.select(Some(prev));
        match self.step {
            PickerStep::Country => self.country_idx = prev,
            PickerStep::Scenario => self.scenario_idx = prev,
        }
    }

    pub fn advance(&mut self) -> bool {
        match self.step {
            PickerStep::Country => {
                self.step = PickerStep::Scenario;
                self.list_state.select(Some(0));
                self.scenario_idx = 0;
                self.rebuild_cache();
                false
            }
            PickerStep::Scenario => !self.filtered_cache.is_empty(),
        }
    }

    pub fn back(&mut self) -> bool {
        if self.step == PickerStep::Scenario {
            self.step = PickerStep::Country;
            self.list_state.select(Some(self.country_idx));
            true
        } else {
            false
        }
    }

    pub fn selected_country(&self) -> Option<&Country> {
        self.countries.get(self.country_idx)
    }

    /// Returns the currently-highlighted scenario, if any.
    pub fn selected_scenario(&self) -> Option<&ScenarioEntry> {
        let id = self.filtered_cache.get(self.scenario_idx)?;
        self.scenarios.iter().find(|s| &s.id == id)
    }

    pub fn filtered_scenarios(&self) -> Vec<&ScenarioEntry> {
        self.filtered_cache
            .iter()
            .filter_map(|id| self.scenarios.iter().find(|s| &s.id == id))
            .collect()
    }
}

pub fn render_picker(frame: &mut Frame, area: Rect, picker: &mut Picker) {
    frame.render_widget(Clear, area);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(1)])
        .split(area);

    let title = match picker.step {
        PickerStep::Country => "PICK A COUNTRY",
        PickerStep::Scenario => "PICK A SCENARIO",
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

    let items: Vec<ListItem> = match picker.step {
        PickerStep::Country => picker
            .countries
            .iter()
            .map(|c| {
                ListItem::new(Line::from(vec![
                    Span::styled(
                        format!("  {} ", c.faction.display_name()),
                        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        c.hint.clone(),
                        Style::default().fg(Color::Gray),
                    ),
                ]))
            })
            .collect(),
        PickerStep::Scenario => picker
            .filtered_scenarios()
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