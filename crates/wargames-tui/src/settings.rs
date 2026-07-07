//! Settings screen — the user-facing place to pick a theme. Live preview:
//! every arrow-key press swaps the active theme immediately so the user
//! can see the palette change before committing. Enter writes the slug
//! to `~/.config/wargames/wargames_settings.json`. Esc reverts to the
//! boot-time theme and exits back to the Game screen.

use crate::theme::{self, Theme};
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::Frame;

/// Settings list state — just the row index + the slug we booted with
/// (so Esc can roll back to it).
pub struct SettingsState {
    pub selected: usize,
    /// Slug of the theme that was active when the screen opened. Esc
    /// restores it.
    pub boot_slug: &'static str,
    pub list_state: ListState,
}

impl SettingsState {
    pub fn new() -> Self {
        let mut ls = ListState::default();
        ls.select(Some(0));
        let boot_slug = theme::current().name;
        // Pre-select the row matching the current theme so arrow-key
        // navigation feels natural — the user sees their current pick
        // highlighted at entry.
        let selected = theme::seeds()
            .iter()
            .position(|t| t.name == boot_slug)
            .unwrap_or(0);
        ls.select(Some(selected));
        Self {
            selected,
            boot_slug,
            list_state: ls,
        }
    }

    pub fn move_up(&mut self) {
        if self.selected == 0 {
            return;
        }
        self.selected -= 1;
        self.list_state.select(Some(self.selected));
    }

    pub fn move_down(&mut self) {
        let max = theme::seeds().len().saturating_sub(1);
        if self.selected >= max {
            return;
        }
        self.selected += 1;
        self.list_state.select(Some(self.selected));
    }

    /// Apply the live-preview swap. Idempotent (theme::set_current is).
    pub fn apply_preview(&self) {
        if let Some(t) = theme::seeds().get(self.selected) {
            theme::set_current(t.clone());
        }
    }

    /// Roll back to whatever theme was active when the screen opened.
    pub fn revert(&self) {
        if let Some(t) = theme::by_name_lookup(self.boot_slug) {
            theme::set_current(t);
        }
    }

    /// The slug chosen at the selected row. Caller persists this.
    pub fn committed_slug(&self) -> &'static str {
        theme::seeds()
            .get(self.selected)
            .map(|t| t.name)
            .unwrap_or(self.boot_slug)
    }
}

/// Render the Settings screen. Re-uses `theme::current()` for styling
/// so the chrome (title, separator, status footer) reflects whatever
/// theme is being previewed. The selected row is highlighted in the
/// inverse color (warn fg on dark bg) just like the Game tab strip.
pub fn render(frame: &mut Frame, area: Rect, state: &mut SettingsState) {
    let t = theme::current();
    frame.render_widget(Clear, area);

    let chunks = ratatui::layout::Layout::default()
        .direction(ratatui::layout::Direction::Vertical)
        .constraints([
            ratatui::layout::Constraint::Length(3), // title
            ratatui::layout::Constraint::Min(8),     // list
            ratatui::layout::Constraint::Length(2),  // footer / hints
        ])
        .split(area);

    // ---- Title bar -----------------------------------------------------
    let title = Paragraph::new(Line::from(Span::styled(
        " SETTINGS · THEME ",
        Style::default()
            .fg(t.title)
            .add_modifier(Modifier::BOLD),
    )))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(t.border)),
    );
    frame.render_widget(title, chunks[0]);

    // ---- Theme list ----------------------------------------------------
    let items: Vec<ListItem> = theme::seeds()
        .iter()
        .map(|seed| {
            let marker = if seed.name == state.boot_slug { " ★" } else { "  " };
            ListItem::new(Line::from(vec![
                Span::styled(marker, Style::default().fg(t.splash_accent)),
                Span::styled(
                    format!("  {:<24}", seed.label),
                    Style::default().fg(t.picker_label).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("({})", seed.name),
                    Style::default().fg(t.picker_dim),
                ),
            ]))
        })
        .collect();
    let list = List::new(items)
        .highlight_style(
            Style::default()
                .bg(t.action_highlight_bg)
                .fg(t.action_highlight_fg)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("» ");
    frame.render_stateful_widget(list, chunks[1], &mut state.list_state);

    // ---- Footer hints --------------------------------------------------
    let footer = Paragraph::new(vec![
        Line::from(Span::styled(
            format!(
                " [↑↓] choose   [Enter] commit + save   [Esc] cancel   boot: {}",
                state.boot_slug
            ),
            Style::default().fg(t.status_dim),
        )),
        Line::from(Span::styled(
            " ★ marks the theme that was loaded at startup",
            Style::default().fg(t.status_dim),
        )),
    ])
    .wrap(Wrap { trim: false });
    frame.render_widget(footer, chunks[2]);
}

/// Look up a theme by slug. Returns `None` if no seeded theme matches;
/// callers fall back to `og_wopr()` themselves.
#[allow(dead_code)] // Used by the planned settings UI when themes become user-selectable.
pub fn by_name_or_default(slug: &str) -> Theme {
    theme::by_name(slug)
}

/// Snapshot of a loaded settings file. Used by the persistence layer
/// to round-trip the slug through disk.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SettingsFile {
    /// Theme slug. Unknown / missing values mean "use the default".
    pub theme: Option<String>,
    /// Schema version for forward-compat. Bumped any time the on-disk
    /// shape changes.
    #[serde(default = "default_version")]
    pub version: u32,
}

fn default_version() -> u32 {
    1
}

impl SettingsFile {
    pub fn new() -> Self {
        Self {
            theme: None,
            version: default_version(),
        }
    }
}

/// `~/.config/wargames/wargames_settings.json` (or `$WARGAMES_CONFIG_DIR`
/// override). The directory is `mkdir -p`'d on first write. We don't
/// add a `dirs` dependency for one path.
pub fn settings_path() -> std::path::PathBuf {
    if let Ok(dir) = std::env::var("WARGAMES_CONFIG_DIR") {
        return std::path::PathBuf::from(dir).join("wargames_settings.json");
    }
    let base = std::env::var("HOME")
        .ok()
        .map(|h| std::path::PathBuf::from(h).join(".config"))
        .unwrap_or_else(|| std::path::PathBuf::from(".config"));
    base.join("wargames").join("wargames_settings.json")
}

/// Load the settings file. Missing file → empty `SettingsFile`.
/// Invalid JSON → empty `SettingsFile` (we never panic on user files).
pub fn load() -> SettingsFile {
    let path = settings_path();
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(_) => return SettingsFile::new(),
    };
    match serde_json::from_slice::<SettingsFile>(&bytes) {
        Ok(s) => s,
        Err(_) => SettingsFile::new(),
    }
}

/// Persist the slug to disk. The write goes through a tmpfile-then-
/// rename to avoid corrupting the file on partial writes; the parent
/// directory is created on demand. Returns `Ok(())` on success.
pub fn save(slug: &str) -> std::io::Result<()> {
    let path = settings_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    let body = SettingsFile {
        theme: Some(slug.to_string()),
        version: default_version(),
    };
    let bytes = serde_json::to_vec_pretty(&body).map_err(std::io::Error::other)?;
    std::fs::write(&tmp, &bytes)?;
    // POSIX-atomic rename overwrites the dest on most systems; on
    // Windows the rename would fail if dest exists, so we remove first.
    if path.exists() {
        let _ = std::fs::remove_file(&path);
    }
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_file_round_trips() {
        let s = SettingsFile {
            theme: Some("tron".into()),
            version: 1,
        };
        let json = serde_json::to_string(&s).unwrap();
        let back: SettingsFile = serde_json::from_str(&json).unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn settings_file_default_version_is_1() {
        // serde-default round-trip.
        let s: SettingsFile = serde_json::from_str("{}").unwrap();
        assert_eq!(s.version, 1);
        assert_eq!(s.theme, None);
    }

    #[test]
    fn settings_path_resolves_under_home_config() {
        // No env override; HOME set by the test runner; the path must
        // include "wargames/wargames_settings.json" and end in .json.
        let p = settings_path();
        assert!(p.ends_with("wargames/wargames_settings.json"));
    }

    #[test]
    fn load_returns_empty_on_missing_file() {
        // We test the no-file branch by pointing WARGAMES_CONFIG_DIR
        // at a non-existent path. CARGO_TEST runs each test in a
        // separate process only if requested; within a single test
        // the env mutation is visible to subsequent calls in the
        // same thread.
        let prev = std::env::var("WARGAMES_CONFIG_DIR").ok();
        let dir = std::env::temp_dir().join(format!(
            "wg-missing-{}",
            std::process::id()
        ));
        std::env::set_var("WARGAMES_CONFIG_DIR", &dir);
        let s = load();
        // Restore env so other tests don't see it.
        match prev.as_deref() {
            Some(v) => std::env::set_var("WARGAMES_CONFIG_DIR", v),
            None => std::env::remove_var("WARGAMES_CONFIG_DIR"),
        }
        assert_eq!(s, SettingsFile::new());
    }

    #[test]
    fn save_and_reload_round_trip() {
        let prev = std::env::var("WARGAMES_CONFIG_DIR").ok();
        let dir = std::env::temp_dir().join(format!(
            "wg-roundtrip-{}-{}",
            std::process::id(),
            line!()
        ));
        std::env::set_var("WARGAMES_CONFIG_DIR", &dir);
        // Save "tron"
        save("tron").expect("save must succeed");
        let loaded = load();
        // Restore env so other tests don't see it.
        match prev.as_deref() {
            Some(v) => std::env::set_var("WARGAMES_CONFIG_DIR", v),
            None => std::env::remove_var("WARGAMES_CONFIG_DIR"),
        }
        assert_eq!(loaded.theme.as_deref(), Some("tron"));
        // Clean up the dir.
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_rejects_unknown_slug_only_by_silently_accepting_it() {
        // We intentionally accept any string into the file — the
        // loader falls back to og_wopr if the slug doesn't resolve.
        // Verify the loose contract: an unknown slug survives a
        // round-trip.
        let prev = std::env::var("WARGAMES_CONFIG_DIR").ok();
        let dir = std::env::temp_dir().join(format!(
            "wg-unknown-{}-{}",
            std::process::id(),
            line!()
        ));
        std::env::set_var("WARGAMES_CONFIG_DIR", &dir);
        save("non-existent-theme").expect("save must succeed");
        let loaded = load();
        match prev.as_deref() {
            Some(v) => std::env::set_var("WARGAMES_CONFIG_DIR", v),
            None => std::env::remove_var("WARGAMES_CONFIG_DIR"),
        }
        assert_eq!(loaded.theme.as_deref(), Some("non-existent-theme"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
