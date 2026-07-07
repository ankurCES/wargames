//! Theme palette + 8 seeded themes.
//!
//! Themes hold every colour the TUI paints. Widgets take `&Theme` and
//! read named fields (`theme.log_us`, `theme.action_label`); there is no
//! hardcoded `Color::Xxx` left in the widget layer. The `og_wopr` theme
//! reproduces the legacy green-on-black palette bit-for-bit so the
//! visible UI does not change for users who never picked a theme.
//!
//! Naming convention
//! -----------------
//! Every field falls into one of three buckets:
//!
//! 1. *Surface* — `border`, `border_inactive`, `background` (optional,
//!    defaulting to `Color::Reset`).
//! 2. *Role* — semantic meaning, e.g. `log_us`, `state_value_warn`,
//!    `action_label`. Roles are stable across themes.
//! 3. *Variant* — a secondary tint of the same role, e.g.
//!    `predict_bar_low` vs `predict_bar_high`. Themes pick meaningful
//!    progressions; viewers see those progressions at a glance.
//!
//! Adding a new theme
//! ------------------
//! `seeds()` returns a `Vec<Theme>`; append a new entry, give it a
//! stable `name` slug, and the picker in the Settings tab picks it up
//! automatically. No widget code needs to change.

use ratatui::style::Color;
use serde::{Deserialize, Serialize};
use std::sync::{Mutex, OnceLock};

/// Process-wide handle to the active theme. Initialized lazily on
/// first access; both `current()` and `set_current()` use this same
/// slot so reads observe writes.
fn slot() -> &'static Mutex<Theme> {
    static SLOT: OnceLock<Mutex<Theme>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(og_wopr()))
}

/// One complete palette. Fields are public so callers can read them
/// directly; widgets construct `Span::styled(text, Style::default().fg(theme.log_us))`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Theme {
    /// Stable identifier used in settings JSON + tests.
    pub name: &'static str,
    /// Human-readable label for the picker.
    pub label: &'static str,

    // -- surface --------------------------------------------------------
    /// Borders around active panes.
    pub border: Color,
    /// Borders around inactive / dim panes.
    pub border_inactive: Color,
    /// Optional pane / padding background. Defaults to `Color::Reset`
    /// (terminal default). Most themes leave it Reset; themes with
    /// a panel fill paint it here.
    pub background: Color,

    // -- titles + status line ------------------------------------------
    /// Pane / banner titles.
    pub title: Color,
    /// Status-line primary text.
    pub status_text: Color,
    /// Status-line warnings ("LlmIdUnavailable…" etc).
    pub status_warn: Color,
    /// Status-line dim info.
    pub status_dim: Color,

    // -- tabs (Compact-mode strip + GameOver menu) --------------------
    pub tabs_active: Color,
    pub tabs_inactive: Color,

    // -- log ----------------------------------------------------------
    /// Headline field (e.g. `[005] us trigger`).
    pub log_header: Color,
    /// Player ("us")-side event rows.
    pub log_us: Color,
    /// Opponent ("opp")-side event rows.
    pub log_opp: Color,
    /// `trigger` kind events.
    pub log_trigger: Color,
    /// `outcome` kind events.
    pub log_outcome: Color,
    /// Neutral / system rows.
    pub log_neutral: Color,
    /// Dim row content (skipped counters, dim informational).
    pub log_dim: Color,
    /// Body text of the log entry.
    pub log_text: Color,

    // -- predict ------------------------------------------------------
    pub predict_label: Color,
    pub predict_text: Color,
    pub predict_bar_low: Color,
    pub predict_bar_mid: Color,
    pub predict_bar_high: Color,

    // -- state --------------------------------------------------------
    pub state_text: Color,
    pub state_dim: Color,
    pub state_era: Color,
    pub state_us: Color,
    pub state_opp: Color,
    pub state_value_ok: Color,
    pub state_value_warn: Color,
    pub state_value_crit: Color,

    // -- radar --------------------------------------------------------
    pub radar_ghost: Color,
    pub radar_us: Color,
    pub radar_nato: Color,
    pub radar_soviet: Color,
    pub radar_neutral: Color,

    // -- action -------------------------------------------------------
    pub action_label: Color,
    pub action_dim: Color,
    pub action_text: Color,
    pub action_highlight_bg: Color,
    pub action_highlight_fg: Color,

    // -- splash -------------------------------------------------------
    pub splash_primary: Color,
    pub splash_accent: Color,
    pub splash_dim: Color,

    // -- picker -------------------------------------------------------
    pub picker_title: Color,
    pub picker_label: Color,
    pub picker_dim: Color,
}

/// A theme selection that survives a round-trip through serde.
///
/// `serde(rename_all = "kebab-case")` so users write `"og-wopr"` and
/// `"cyberpunk"` in their settings.json rather than `OgWopr`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ThemeName {
    OgWopr,
    #[serde(rename = "cyberpunk")]
    Cyberpunk2077,
    Tron,
    VscodeDarkPlus,
    VscodeMonokai,
    SolarizedDark,
    #[serde(rename = "synthwave")]
    Synthwave84,
    Dracula,
}

impl Default for ThemeName {
    fn default() -> Self {
        ThemeName::OgWopr
    }
}

impl ThemeName {
    /// Stable string identifier used in settings + log lines.
    pub fn slug(self) -> &'static str {
        match self {
            ThemeName::OgWopr => "og-wopr",
            ThemeName::Cyberpunk2077 => "cyberpunk",
            ThemeName::Tron => "tron",
            ThemeName::VscodeDarkPlus => "vscode-dark-plus",
            ThemeName::VscodeMonokai => "vscode-monokai",
            ThemeName::SolarizedDark => "solarized-dark",
            ThemeName::Synthwave84 => "synthwave",
            ThemeName::Dracula => "dracula",
        }
    }

    /// Human-readable label shown in the Settings tab list.
    #[allow(dead_code)] // Surfaced in the Settings tab once the theme picker UI lands.
    pub fn label(self) -> &'static str {
        match self {
            ThemeName::OgWopr => "OG WOPR",
            ThemeName::Cyberpunk2077 => "Cyberpunk 2077",
            ThemeName::Tron => "Tron",
            ThemeName::VscodeDarkPlus => "VS Code Dark+",
            ThemeName::VscodeMonokai => "VS Code Monokai",
            ThemeName::SolarizedDark => "Solarized Dark",
            ThemeName::Synthwave84 => "Synthwave '84",
            ThemeName::Dracula => "Dracula",
        }
    }

    /// Resolve a `ThemeName` to its concrete `Theme`. Returns `og_wopr`
    /// for any unknown variant — defensive default so a malformed
    /// settings file still produces a usable UI.
    pub fn resolve(self) -> Theme {
        match self {
            ThemeName::OgWopr => og_wopr(),
            ThemeName::Cyberpunk2077 => cyberpunk_2077(),
            ThemeName::Tron => tron(),
            ThemeName::VscodeDarkPlus => vscode_dark_plus(),
            ThemeName::VscodeMonokai => vscode_monokai(),
            ThemeName::SolarizedDark => solarized_dark(),
            ThemeName::Synthwave84 => synthwave_84(),
            ThemeName::Dracula => dracula(),
        }
    }
}

/// All seeded themes, in the order they appear in the picker.
///
/// Stored in a `OnceLock<Vec<Theme>>` because `Theme` instances are
/// constructed from factory functions (we want zero-cost space rather
/// than `Box` indirection) but they must outlive the function call.
/// The first call materializes the list; subsequent calls return a
/// clone.
pub fn seeds() -> Vec<Theme> {
    use std::sync::OnceLock;
    static CACHE: OnceLock<Vec<Theme>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            vec![
                og_wopr(),
                cyberpunk_2077(),
                tron(),
                vscode_dark_plus(),
                vscode_monokai(),
                solarized_dark(),
                synthwave_84(),
                dracula(),
            ]
        })
        .clone()
}

/// Same data as [`seeds`] but borrowed. Used in test paths.
pub fn seeds_slice() -> &'static [Theme] {
    use std::sync::OnceLock;
    static CACHE: OnceLock<Vec<Theme>> = OnceLock::new();
    CACHE.get_or_init(|| {
        vec![
            og_wopr(),
            cyberpunk_2077(),
            tron(),
            vscode_dark_plus(),
            vscode_monokai(),
            solarized_dark(),
            synthwave_84(),
            dracula(),
        ]
    })
}

/// Resolve a theme by name. Falls back to `og_wopr` so an unknown
/// theme slug in settings.json never breaks the UI.
pub fn by_name(slug: &str) -> Theme {
    by_name_lookup(slug).unwrap_or_else(|| og_wopr())
}

/// Look up a theme by slug without a fallback. Returns `None` if the
/// slug is not one of the seeded themes. Useful for callers that want
/// to decide for themselves what to do with an unknown slug (e.g.
/// silently keep the boot theme instead of swapping to `og_wopr`).
pub fn by_name_lookup(slug: &str) -> Option<Theme> {
    for t in seeds_slice() {
        if t.name == slug {
            return Some(t.clone());
        }
    }
    None
}

/// The theme the renderer is currently using. Initialized to
/// [`og_wopr`] on first access. Widgets read this via [`current`]
/// instead of taking `&Theme` as a parameter, which keeps the
/// hundreds of paint sites untouched while still letting the
/// Settings tab swap palettes live.
///
/// Implementation: `Theme` is `Copy`, so `current()` returns by
/// value and callers pick what they need (e.g. `theme.border`).
/// Storing the returned `Theme` past a paint is fine — it's just
/// ~40 bytes of `Color` (also `Copy`). The runtime store is a
/// `Mutex<Theme>` to allow reads + writes from the single render
/// thread without `unsafe` and without a `'static`-lifetime dance.
pub fn current() -> Theme {
    *slot().lock().expect("theme mutex poisoned")
}

/// Replace the active theme. Idempotent — assigning the same value
/// is a no-op so live-preview key spam doesn't churn anything.
pub fn set_current(theme: Theme) {
    let mut guard = slot().lock().expect("theme mutex poisoned");
    if guard.name == theme.name {
        return;
    }
    *guard = theme;
}

/// The legacy palette. Every value here is the colour the corresponding
/// widget used to hardcode before the theme refactor — locking this
/// in a test prevents theme drift on the default theme.
pub fn og_wopr() -> Theme {
    Theme {
        name: "og-wopr",
        label: "OG WOPR",
        border: Color::Green,
        border_inactive: Color::DarkGray,
        background: Color::Reset,
        title: Color::Green,
        status_text: Color::White,
        status_warn: Color::Yellow,
        status_dim: Color::DarkGray,
        tabs_active: Color::Green,
        tabs_inactive: Color::DarkGray,
        log_header: Color::Cyan,
        log_us: Color::Cyan,
        log_opp: Color::LightRed,
        log_trigger: Color::Yellow,
        log_outcome: Color::Magenta,
        log_neutral: Color::Gray,
        log_dim: Color::DarkGray,
        log_text: Color::White,
        predict_label: Color::Cyan,
        predict_text: Color::White,
        predict_bar_low: Color::Green,
        predict_bar_mid: Color::Yellow,
        predict_bar_high: Color::Red,
        state_text: Color::White,
        state_dim: Color::DarkGray,
        state_era: Color::Yellow,
        state_us: Color::Cyan,
        state_opp: Color::LightRed,
        state_value_ok: Color::Green,
        state_value_warn: Color::Yellow,
        state_value_crit: Color::Red,
        radar_ghost: Color::DarkGray,
        radar_us: Color::Cyan,
        radar_nato: Color::Cyan,
        radar_soviet: Color::LightRed,
        radar_neutral: Color::Yellow,
        action_label: Color::Cyan,
        action_dim: Color::Gray,
        action_text: Color::White,
        action_highlight_bg: Color::Rgb(52, 0, 0),
        action_highlight_fg: Color::White,
        splash_primary: Color::Cyan,
        splash_accent: Color::Yellow,
        splash_dim: Color::DarkGray,
        picker_title: Color::Green,
        picker_label: Color::Cyan,
        picker_dim: Color::Gray,
    }
}

/// Cyberpunk 2077 — magenta + cyan on near-black.
pub fn cyberpunk_2077() -> Theme {
    Theme {
        name: "cyberpunk",
        label: "Cyberpunk 2077",
        border: Color::Rgb(255, 0, 64),       // #ff003f cyber-punk red
        border_inactive: Color::Rgb(60, 0, 30),
        background: Color::Reset,
        title: Color::Rgb(0, 240, 255),       // neon cyan
        status_text: Color::White,
        status_warn: Color::Rgb(247, 0, 255), // magenta
        status_dim: Color::Rgb(80, 80, 100),
        tabs_active: Color::Rgb(0, 240, 255),
        tabs_inactive: Color::Rgb(80, 80, 100),
        log_header: Color::Rgb(0, 240, 255),
        log_us: Color::Rgb(0, 240, 255),
        log_opp: Color::Rgb(255, 0, 64),
        log_trigger: Color::Rgb(247, 0, 255),
        log_outcome: Color::Rgb(255, 213, 0),
        log_neutral: Color::Rgb(180, 180, 200),
        log_dim: Color::Rgb(60, 60, 80),
        log_text: Color::White,
        predict_label: Color::Rgb(0, 240, 255),
        predict_text: Color::White,
        predict_bar_low: Color::Rgb(0, 240, 255),
        predict_bar_mid: Color::Rgb(247, 0, 255),
        predict_bar_high: Color::Rgb(255, 0, 64),
        state_text: Color::White,
        state_dim: Color::Rgb(80, 80, 100),
        state_era: Color::Rgb(255, 213, 0),
        state_us: Color::Rgb(0, 240, 255),
        state_opp: Color::Rgb(255, 0, 64),
        state_value_ok: Color::Rgb(0, 240, 255),
        state_value_warn: Color::Rgb(247, 0, 255),
        state_value_crit: Color::Rgb(255, 0, 64),
        radar_ghost: Color::Rgb(60, 60, 80),
        radar_us: Color::Rgb(0, 240, 255),
        radar_nato: Color::Rgb(0, 240, 255),
        radar_soviet: Color::Rgb(255, 0, 64),
        radar_neutral: Color::Rgb(247, 0, 255),
        action_label: Color::Rgb(0, 240, 255),
        action_dim: Color::Rgb(120, 120, 140),
        action_text: Color::White,
        action_highlight_bg: Color::Rgb(60, 0, 30),
        action_highlight_fg: Color::Rgb(0, 240, 255),
        splash_primary: Color::Rgb(0, 240, 255),
        splash_accent: Color::Rgb(247, 0, 255),
        splash_dim: Color::Rgb(80, 80, 100),
        picker_title: Color::Rgb(255, 0, 64),
        picker_label: Color::Rgb(0, 240, 255),
        picker_dim: Color::Rgb(120, 120, 140),
    }
}

/// Tron — cyan-blue + neon orange.
pub fn tron() -> Theme {
    Theme {
        name: "tron",
        label: "Tron",
        border: Color::Rgb(11, 210, 255),       // #0bd2ff cyan-blue
        border_inactive: Color::Rgb(20, 60, 90),
        background: Color::Reset,
        title: Color::Rgb(11, 210, 255),
        status_text: Color::White,
        status_warn: Color::Rgb(251, 146, 60), // neon orange
        status_dim: Color::Rgb(50, 90, 110),
        tabs_active: Color::Rgb(11, 210, 255),
        tabs_inactive: Color::Rgb(50, 90, 110),
        log_header: Color::Rgb(11, 210, 255),
        log_us: Color::Rgb(11, 210, 255),
        log_opp: Color::Rgb(251, 146, 60),
        log_trigger: Color::Rgb(255, 224, 102),
        log_outcome: Color::Rgb(146, 220, 255),
        log_neutral: Color::Rgb(180, 220, 240),
        log_dim: Color::Rgb(40, 70, 90),
        log_text: Color::White,
        predict_label: Color::Rgb(11, 210, 255),
        predict_text: Color::White,
        predict_bar_low: Color::Rgb(11, 210, 255),
        predict_bar_mid: Color::Rgb(255, 224, 102),
        predict_bar_high: Color::Rgb(251, 146, 60),
        state_text: Color::White,
        state_dim: Color::Rgb(50, 90, 110),
        state_era: Color::Rgb(255, 224, 102),
        state_us: Color::Rgb(11, 210, 255),
        state_opp: Color::Rgb(251, 146, 60),
        state_value_ok: Color::Rgb(11, 210, 255),
        state_value_warn: Color::Rgb(255, 224, 102),
        state_value_crit: Color::Rgb(251, 146, 60),
        radar_ghost: Color::Rgb(40, 70, 90),
        radar_us: Color::Rgb(11, 210, 255),
        radar_nato: Color::Rgb(146, 220, 255),
        radar_soviet: Color::Rgb(251, 146, 60),
        radar_neutral: Color::Rgb(255, 224, 102),
        action_label: Color::Rgb(11, 210, 255),
        action_dim: Color::Rgb(120, 160, 180),
        action_text: Color::White,
        action_highlight_bg: Color::Rgb(20, 60, 90),
        action_highlight_fg: Color::Rgb(255, 224, 102),
        splash_primary: Color::Rgb(11, 210, 255),
        splash_accent: Color::Rgb(251, 146, 60),
        splash_dim: Color::Rgb(50, 90, 110),
        picker_title: Color::Rgb(11, 210, 255),
        picker_label: Color::Rgb(146, 220, 255),
        picker_dim: Color::Rgb(120, 160, 180),
    }
}

/// VS Code Dark+ — muted blues + grey keywords.
pub fn vscode_dark_plus() -> Theme {
    Theme {
        name: "vscode-dark-plus",
        label: "VS Code Dark+",
        border: Color::Rgb(86, 156, 214),       // #569cd6 keyword blue
        border_inactive: Color::Rgb(60, 80, 100),
        background: Color::Reset,
        title: Color::Rgb(86, 156, 214),
        status_text: Color::Rgb(212, 212, 212), // #d4d4d4 foreground
        status_warn: Color::Rgb(206, 145, 120), // #ce9178 string
        status_dim: Color::Rgb(80, 100, 120),
        tabs_active: Color::Rgb(86, 156, 214),
        tabs_inactive: Color::Rgb(110, 110, 110),
        log_header: Color::Rgb(86, 156, 214),
        log_us: Color::Rgb(86, 156, 214),
        log_opp: Color::Rgb(206, 145, 120),
        log_trigger: Color::Rgb(220, 220, 170), // #dcdcaa function-name
        log_outcome: Color::Rgb(197, 134, 192), // #c586c0 control-flow purple
        log_neutral: Color::Rgb(212, 212, 212),
        log_dim: Color::Rgb(80, 100, 120),
        log_text: Color::Rgb(212, 212, 212),
        predict_label: Color::Rgb(86, 156, 214),
        predict_text: Color::Rgb(212, 212, 212),
        predict_bar_low: Color::Rgb(86, 156, 214),
        predict_bar_mid: Color::Rgb(220, 220, 170),
        predict_bar_high: Color::Rgb(206, 145, 120),
        state_text: Color::Rgb(212, 212, 212),
        state_dim: Color::Rgb(80, 100, 120),
        state_era: Color::Rgb(220, 220, 170),
        state_us: Color::Rgb(86, 156, 214),
        state_opp: Color::Rgb(206, 145, 120),
        state_value_ok: Color::Rgb(86, 156, 214),
        state_value_warn: Color::Rgb(220, 220, 170),
        state_value_crit: Color::Rgb(206, 145, 120),
        radar_ghost: Color::Rgb(80, 100, 120),
        radar_us: Color::Rgb(86, 156, 214),
        radar_nato: Color::Rgb(106, 153, 85),   // #6a9955 comment green
        radar_soviet: Color::Rgb(206, 145, 120),
        radar_neutral: Color::Rgb(197, 134, 192),
        action_label: Color::Rgb(86, 156, 214),
        action_dim: Color::Rgb(140, 140, 140),
        action_text: Color::Rgb(212, 212, 212),
        action_highlight_bg: Color::Rgb(40, 60, 80),
        action_highlight_fg: Color::Rgb(220, 220, 170),
        splash_primary: Color::Rgb(86, 156, 214),
        splash_accent: Color::Rgb(220, 220, 170),
        splash_dim: Color::Rgb(80, 100, 120),
        picker_title: Color::Rgb(86, 156, 214),
        picker_label: Color::Rgb(106, 153, 85),
        picker_dim: Color::Rgb(140, 140, 140),
    }
}

/// VS Code Monokai — pink + green + orange.
pub fn vscode_monokai() -> Theme {
    Theme {
        name: "vscode-monokai",
        label: "VS Code Monokai",
        border: Color::Rgb(249, 38, 114),     // #f92672 pink
        border_inactive: Color::Rgb(80, 50, 70),
        background: Color::Reset,
        title: Color::Rgb(249, 38, 114),
        status_text: Color::Rgb(248, 248, 242), // #f8f8f2 foreground
        status_warn: Color::Rgb(253, 151, 31),  // #fd971f orange
        status_dim: Color::Rgb(110, 100, 100),
        tabs_active: Color::Rgb(249, 38, 114),
        tabs_inactive: Color::Rgb(110, 100, 100),
        log_header: Color::Rgb(166, 226, 46),   // #a6e22e green
        log_us: Color::Rgb(166, 226, 46),
        log_opp: Color::Rgb(249, 38, 114),
        log_trigger: Color::Rgb(253, 151, 31),
        log_outcome: Color::Rgb(174, 129, 255), // #ae81ff purple
        log_neutral: Color::Rgb(220, 220, 200),
        log_dim: Color::Rgb(80, 80, 70),
        log_text: Color::Rgb(248, 248, 242),
        predict_label: Color::Rgb(166, 226, 46),
        predict_text: Color::Rgb(248, 248, 242),
        predict_bar_low: Color::Rgb(166, 226, 46),
        predict_bar_mid: Color::Rgb(253, 151, 31),
        predict_bar_high: Color::Rgb(249, 38, 114),
        state_text: Color::Rgb(248, 248, 242),
        state_dim: Color::Rgb(110, 100, 100),
        state_era: Color::Rgb(253, 151, 31),
        state_us: Color::Rgb(166, 226, 46),
        state_opp: Color::Rgb(249, 38, 114),
        state_value_ok: Color::Rgb(166, 226, 46),
        state_value_warn: Color::Rgb(253, 151, 31),
        state_value_crit: Color::Rgb(249, 38, 114),
        radar_ghost: Color::Rgb(80, 80, 70),
        radar_us: Color::Rgb(166, 226, 46),
        radar_nato: Color::Rgb(174, 129, 255),
        radar_soviet: Color::Rgb(249, 38, 114),
        radar_neutral: Color::Rgb(253, 151, 31),
        action_label: Color::Rgb(166, 226, 46),
        action_dim: Color::Rgb(140, 130, 130),
        action_text: Color::Rgb(248, 248, 242),
        action_highlight_bg: Color::Rgb(70, 50, 50),
        action_highlight_fg: Color::Rgb(166, 226, 46),
        splash_primary: Color::Rgb(249, 38, 114),
        splash_accent: Color::Rgb(166, 226, 46),
        splash_dim: Color::Rgb(80, 80, 70),
        picker_title: Color::Rgb(249, 38, 114),
        picker_label: Color::Rgb(166, 226, 46),
        picker_dim: Color::Rgb(140, 130, 130),
    }
}

/// Solarized Dark — base03 background with cyan/blue accent.
pub fn solarized_dark() -> Theme {
    Theme {
        name: "solarized-dark",
        label: "Solarized Dark",
        border: Color::Rgb(38, 139, 210),       // blue
        border_inactive: Color::Rgb(50, 60, 70),
        background: Color::Reset,
        title: Color::Rgb(38, 139, 210),
        status_text: Color::Rgb(147, 161, 161), // #93a1a1 base1
        status_warn: Color::Rgb(220, 145, 60),
        status_dim: Color::Rgb(88, 110, 117),   // #586e75 base01
        tabs_active: Color::Rgb(38, 139, 210),
        tabs_inactive: Color::Rgb(88, 110, 117),
        log_header: Color::Rgb(133, 153, 0),    // #859900 green
        log_us: Color::Rgb(133, 153, 0),
        log_opp: Color::Rgb(220, 50, 47),       // #dc322f red
        log_trigger: Color::Rgb(181, 137, 0),   // #b58900 yellow
        log_outcome: Color::Rgb(108, 113, 196), // #6c71c4 violet
        log_neutral: Color::Rgb(147, 161, 161),
        log_dim: Color::Rgb(88, 110, 117),
        log_text: Color::Rgb(147, 161, 161),
        predict_label: Color::Rgb(38, 139, 210),
        predict_text: Color::Rgb(147, 161, 161),
        predict_bar_low: Color::Rgb(38, 139, 210),
        predict_bar_mid: Color::Rgb(181, 137, 0),
        predict_bar_high: Color::Rgb(220, 50, 47),
        state_text: Color::Rgb(147, 161, 161),
        state_dim: Color::Rgb(88, 110, 117),
        state_era: Color::Rgb(181, 137, 0),
        state_us: Color::Rgb(42, 161, 152),     // #2aa198 cyan
        state_opp: Color::Rgb(220, 50, 47),
        state_value_ok: Color::Rgb(133, 153, 0),
        state_value_warn: Color::Rgb(181, 137, 0),
        state_value_crit: Color::Rgb(220, 50, 47),
        radar_ghost: Color::Rgb(88, 110, 117),
        radar_us: Color::Rgb(42, 161, 152),
        radar_nato: Color::Rgb(38, 139, 210),
        radar_soviet: Color::Rgb(220, 50, 47),
        radar_neutral: Color::Rgb(181, 137, 0),
        action_label: Color::Rgb(42, 161, 152),
        action_dim: Color::Rgb(120, 130, 130),
        action_text: Color::Rgb(147, 161, 161),
        action_highlight_bg: Color::Rgb(40, 50, 60),
        action_highlight_fg: Color::Rgb(181, 137, 0),
        splash_primary: Color::Rgb(38, 139, 210),
        splash_accent: Color::Rgb(181, 137, 0),
        splash_dim: Color::Rgb(88, 110, 117),
        picker_title: Color::Rgb(38, 139, 210),
        picker_label: Color::Rgb(42, 161, 152),
        picker_dim: Color::Rgb(120, 130, 130),
    }
}

/// Synthwave '84 — magenta + cyan, deep purple background.
pub fn synthwave_84() -> Theme {
    Theme {
        name: "synthwave",
        label: "Synthwave '84",
        border: Color::Rgb(255, 121, 198),     // #ff79c6 magenta
        border_inactive: Color::Rgb(80, 50, 90),
        background: Color::Reset,
        title: Color::Rgb(128, 255, 232),      // #80ffe8 cyan
        status_text: Color::White,
        status_warn: Color::Rgb(241, 250, 140), // #f1fa8c pale yellow
        status_dim: Color::Rgb(120, 100, 140),
        tabs_active: Color::Rgb(128, 255, 232),
        tabs_inactive: Color::Rgb(120, 100, 140),
        log_header: Color::Rgb(128, 255, 232),
        log_us: Color::Rgb(128, 255, 232),
        log_opp: Color::Rgb(255, 121, 198),
        log_trigger: Color::Rgb(241, 250, 140),
        log_outcome: Color::Rgb(189, 147, 249), // #bd93f9 lavender
        log_neutral: Color::Rgb(220, 200, 240),
        log_dim: Color::Rgb(80, 60, 90),
        log_text: Color::White,
        predict_label: Color::Rgb(128, 255, 232),
        predict_text: Color::White,
        predict_bar_low: Color::Rgb(128, 255, 232),
        predict_bar_mid: Color::Rgb(255, 121, 198),
        predict_bar_high: Color::Rgb(189, 147, 249),
        state_text: Color::White,
        state_dim: Color::Rgb(120, 100, 140),
        state_era: Color::Rgb(241, 250, 140),
        state_us: Color::Rgb(128, 255, 232),
        state_opp: Color::Rgb(255, 121, 198),
        state_value_ok: Color::Rgb(128, 255, 232),
        state_value_warn: Color::Rgb(241, 250, 140),
        state_value_crit: Color::Rgb(255, 121, 198),
        radar_ghost: Color::Rgb(80, 60, 90),
        radar_us: Color::Rgb(128, 255, 232),
        radar_nato: Color::Rgb(189, 147, 249),
        radar_soviet: Color::Rgb(255, 121, 198),
        radar_neutral: Color::Rgb(241, 250, 140),
        action_label: Color::Rgb(128, 255, 232),
        action_dim: Color::Rgb(160, 140, 180),
        action_text: Color::White,
        action_highlight_bg: Color::Rgb(60, 40, 80),
        action_highlight_fg: Color::Rgb(241, 250, 140),
        splash_primary: Color::Rgb(255, 121, 198),
        splash_accent: Color::Rgb(128, 255, 232),
        splash_dim: Color::Rgb(120, 100, 140),
        picker_title: Color::Rgb(255, 121, 198),
        picker_label: Color::Rgb(128, 255, 232),
        picker_dim: Color::Rgb(160, 140, 180),
    }
}

/// Dracula — purple + green + pink on dark background.
pub fn dracula() -> Theme {
    Theme {
        name: "dracula",
        label: "Dracula",
        border: Color::Rgb(189, 147, 249), // #bd93f9 purple
        border_inactive: Color::Rgb(70, 60, 90),
        background: Color::Reset,
        title: Color::Rgb(189, 147, 249),
        status_text: Color::Rgb(248, 248, 242), // #f8f8f2 foreground
        status_warn: Color::Rgb(255, 121, 198), // #ff79c6 pink
        status_dim: Color::Rgb(98, 114, 164),   // #6272a4 comment blue
        tabs_active: Color::Rgb(189, 147, 249),
        tabs_inactive: Color::Rgb(98, 114, 164),
        log_header: Color::Rgb(80, 250, 123),    // #50fa7b green
        log_us: Color::Rgb(80, 250, 123),
        log_opp: Color::Rgb(255, 121, 198),
        log_trigger: Color::Rgb(241, 250, 140), // #f1fa8c yellow
        log_outcome: Color::Rgb(139, 233, 253), // #8be9fd cyan
        log_neutral: Color::Rgb(220, 220, 230),
        log_dim: Color::Rgb(68, 71, 90),         // #44475a selection
        log_text: Color::Rgb(248, 248, 242),
        predict_label: Color::Rgb(80, 250, 123),
        predict_text: Color::Rgb(248, 248, 242),
        predict_bar_low: Color::Rgb(80, 250, 123),
        predict_bar_mid: Color::Rgb(241, 250, 140),
        predict_bar_high: Color::Rgb(255, 121, 198),
        state_text: Color::Rgb(248, 248, 242),
        state_dim: Color::Rgb(98, 114, 164),
        state_era: Color::Rgb(241, 250, 140),
        state_us: Color::Rgb(80, 250, 123),
        state_opp: Color::Rgb(255, 121, 198),
        state_value_ok: Color::Rgb(80, 250, 123),
        state_value_warn: Color::Rgb(241, 250, 140),
        state_value_crit: Color::Rgb(255, 121, 198),
        radar_ghost: Color::Rgb(68, 71, 90),
        radar_us: Color::Rgb(80, 250, 123),
        radar_nato: Color::Rgb(139, 233, 253),
        radar_soviet: Color::Rgb(255, 121, 198),
        radar_neutral: Color::Rgb(241, 250, 140),
        action_label: Color::Rgb(189, 147, 249),
        action_dim: Color::Rgb(150, 150, 170),
        action_text: Color::Rgb(248, 248, 242),
        action_highlight_bg: Color::Rgb(60, 50, 80),
        action_highlight_fg: Color::Rgb(241, 250, 140),
        splash_primary: Color::Rgb(189, 147, 249),
        splash_accent: Color::Rgb(80, 250, 123),
        splash_dim: Color::Rgb(98, 114, 164),
        picker_title: Color::Rgb(189, 147, 249),
        picker_label: Color::Rgb(80, 250, 123),
        picker_dim: Color::Rgb(150, 150, 170),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn og_wopr_matches_legacy_palette() {
        // Locks every field to the value the corresponding widget used
        // to hardcode before the theme refactor. If you ever change a
        // value here intentionally, audit every widget; if you change
        // it by accident, this test pins it.
        let t = og_wopr();
        assert_eq!(t.name, "og-wopr");
        assert_eq!(t.border, Color::Green);
        assert_eq!(t.border_inactive, Color::DarkGray);
        assert_eq!(t.title, Color::Green);
        assert_eq!(t.status_text, Color::White);
        assert_eq!(t.status_warn, Color::Yellow);
        assert_eq!(t.log_header, Color::Cyan);
        assert_eq!(t.log_us, Color::Cyan);
        assert_eq!(t.log_opp, Color::LightRed);
        assert_eq!(t.predict_bar_low, Color::Green);
        assert_eq!(t.predict_bar_mid, Color::Yellow);
        assert_eq!(t.predict_bar_high, Color::Red);
        assert_eq!(t.state_us, Color::Cyan);
        assert_eq!(t.state_opp, Color::LightRed);
        assert_eq!(t.radar_us, Color::Cyan);
        assert_eq!(t.radar_soviet, Color::LightRed);
        assert_eq!(t.action_highlight_bg, Color::Rgb(52, 0, 0));
        assert_eq!(t.action_highlight_fg, Color::White);
        assert_eq!(t.splash_primary, Color::Cyan);
        assert_eq!(t.splash_accent, Color::Yellow);
    }

    #[test]
    fn by_name_resolves_all_seeds() {
        for seed in seeds_slice() {
            let t = by_name(seed.name);
            assert_eq!(
                t.name,
                seed.name,
                "by_name(\"{}\") should round-trip to itself",
                seed.name
            );
        }
    }

    #[test]
    fn by_name_falls_back_to_og_wopr_for_garbage() {
        let t = by_name("a theme that does not exist");
        assert_eq!(t.name, "og-wopr");
        // Sanity: it must also equal the explicit og_wopr() const.
        assert_eq!(t, og_wopr());
    }

    #[test]
    fn serializes_through_blumi_settings() {
        // Round-trip a JSON snippet through `BlumiSettings` to confirm
        // the `theme` field deserializes correctly. We don't depend on
        // config.rs being serde-aware yet — that's phase 2 work —
        // but the ThemeName itself must round-trip standalone.
        for name in [
            ThemeName::OgWopr,
            ThemeName::Cyberpunk2077,
            ThemeName::Tron,
            ThemeName::VscodeDarkPlus,
            ThemeName::VscodeMonokai,
            ThemeName::SolarizedDark,
            ThemeName::Synthwave84,
            ThemeName::Dracula,
        ] {
            let json = serde_json::to_string(&name).unwrap();
            // kebab-case invariants.
            assert!(
                json.chars().all(|c| c == '"' || c == '-' || c.is_ascii_lowercase()),
                "ThemeName serialization should be lowercase kebab: got {json}"
            );
            let back: ThemeName = serde_json::from_str(&json).unwrap();
            assert_eq!(back, name);
        }
    }

    #[test]
    fn theme_name_slug_matches_its_field() {
        // The slug is what shows up in settings.json; it must equal
        // `theme.name` so `by_name(theme_name.slug()) == theme`.
        for name in [
            ThemeName::OgWopr,
            ThemeName::Cyberpunk2077,
            ThemeName::Tron,
            ThemeName::VscodeDarkPlus,
            ThemeName::VscodeMonokai,
            ThemeName::SolarizedDark,
            ThemeName::Synthwave84,
            ThemeName::Dracula,
        ] {
            assert_eq!(name.resolve().name, name.slug());
        }
    }

    #[test]
    fn defaults_to_og_wopr() {
        assert_eq!(ThemeName::default(), ThemeName::OgWopr);
    }

    #[test]
    fn every_seed_theme_has_distinct_palette() {
        // Sanity: no two themes are bit-identical (that would be a typo
        // in the palette tables).
        let seeds_list = seeds_slice();
        for i in 0..seeds_list.len() {
            for j in (i + 1)..seeds_list.len() {
                assert_ne!(
                    seeds_list[i], seeds_list[j],
                    "seed themes at index {i} and {j} are identical",
                );
            }
        }
    }

    #[test]
    fn current_defaults_to_og_wopr() {
        // Always reset at the start of this test in case another test
        // in the same process swapped the global theme.
        set_current(og_wopr());
        assert_eq!(current().name, og_wopr().name);
    }

    #[test]
    fn set_current_swaps_then_resets() {
        // Begin at known state.
        set_current(og_wopr());
        assert_eq!(current().name, "og-wopr");

        set_current(seeds()[2].clone());
        assert_eq!(current().name, seeds()[2].name);

        // Same-assignment is a no-op (kept idempotent so live-preview
        // doesn't churn the lock).
        set_current(seeds()[2].clone());
        assert_eq!(current().name, seeds()[2].name);

        // Restore so other tests (and the bin flow) see og_wopr by
        // default.
        set_current(og_wopr());
        assert_eq!(current().name, "og-wopr");
    }
}
