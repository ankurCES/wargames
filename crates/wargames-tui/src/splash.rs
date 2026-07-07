//! Splash — a 5-second "WAR GAMES OG" banner that paints the frame.
//!
//! The figlet is unconditionally 5 rows × 92 columns wide — that's too
//! wide for narrow terminals. At &lt; 96 cols we render a compact
//! 3-row block-letter "WARGAMES" header instead; at &lt; 60 cols we drop
//! the subtitle flair and only emit the headline + countdown so the
//! splash never overflows.

use crate::theme;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use ratatui::Frame;

const SPLASH_ART_WIDE: &str = r#"
 _    _    ___     __ ______ ____   ___  ____   ____   ___  __  __ ____  ____
| |  / \  / _ \   / /| ____|  _ \ / _ \|  _ \ / ___| |  _ \|  \/  / __ )|  _ \
| | / _ \| | | | / /_|  _| | |_) | | | | |_) | |  _  | |_) | |\/| |  _ \| |_) |
| |/ ___ \ |_| |/ ___ | |___|  _ <| |_| |  _ <| |_| | |  __/| |  | | |_) |  _ <
|__/_/   \_\___/_/   |_____|_| \_\\___/|_| \_\\____| |_|   |_|  |_|____/|_| \_\
"#;

const SPLASH_ART_COMPACT: &str = r#"
 __        __   ___  __  __   __  __  __ ___  __  __   ___
 \ \      / /  / _ \ \ \/ /  / /  \ \/ / __ )/ _ \ \  / /  /
  \ \ /\ / /  | (_) | \  /  / /    \  /  _ \ \  / \/ / /
   \ V  V /    \___/   /_/  /_/_   |  \ \  /_) | |__/\  / /___
    \_/\_/                          \_\_\____/   \_\_\\____/
"#;

#[derive(Copy, Clone, PartialEq, Eq)]
enum SplashVariant {
    /// Full 5-line banner — requires ≥ 96 columns.
    Wide,
    /// 3-row compact banner — fits in ≥ 60 columns.
    Compact,
    /// Tiny banner — drop everything but the title and countdown.
    Tiny,
}

fn classify_splash(area: Rect) -> SplashVariant {
    if area.width >= 96 {
        SplashVariant::Wide
    } else if area.width >= 60 {
        SplashVariant::Compact
    } else {
        SplashVariant::Tiny
    }
}

pub fn render_splash(frame: &mut Frame, area: Rect, seconds_remaining: u8) {
    frame.render_widget(Clear, area);
    let theme = theme::current();
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border))
        .title(Span::styled(
            " WARGAMES / WOPR ",
            Style::default()
                .fg(theme.title)
                .add_modifier(Modifier::BOLD),
        ));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let cyan = Style::default().fg(theme.splash_primary).add_modifier(Modifier::BOLD);
    let dim = Style::default().fg(theme.splash_dim);
    let variant = classify_splash(inner);

    let art = match variant {
        SplashVariant::Wide => SPLASH_ART_WIDE,
        SplashVariant::Compact => SPLASH_ART_COMPACT,
        SplashVariant::Tiny => "", // skip the figlet on tiny screens
    };

    let mut lines: Vec<Line> = art
        .lines()
        .map(|l| Line::from(Span::styled(l.to_string(), cyan)))
        .collect();

    if variant != SplashVariant::Tiny {
        lines.push(Line::from(""));
    }
    lines.push(Line::from(Span::styled(
        "Strategic Defense Initiative Online",
        Style::default().fg(theme.splash_accent).add_modifier(Modifier::BOLD),
    )));
    // Skip the smaller descriptor text on Tiny — every row counts.
    if variant != SplashVariant::Tiny {
        lines.push(Line::from(Span::styled(
            "All scenarios derived from real-world events.",
            Style::default().fg(theme.splash_dim),
        )));
        lines.push(Line::from(Span::styled(
            "Predictions update each turn from a Monte Carlo roll.",
            Style::default().fg(theme.splash_dim),
        )));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        format!(
            "Press any key to skip — splash ends in {}s",
            seconds_remaining
        ),
        dim,
    )));
    let p = Paragraph::new(lines).wrap(Wrap { trim: false });
    frame.render_widget(p, inner);
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    use ratatui::{TerminalOptions, Viewport};

    fn render_at(w: u16, h: u16, secs: u8) {
        let backend = TestBackend::new(w, h);
        let mut terminal =
            Terminal::with_options(backend, TerminalOptions { viewport: Viewport::Fullscreen })
                .expect("terminal");
        terminal
            .draw(|f| render_splash(f, f.area(), secs))
            .unwrap_or_else(|e| panic!("render at {w}x{h} failed: {e}"));
    }

    #[test]
    fn render_at_wide_does_not_panic() {
        render_at(120, 24, 3);
    }
    #[test]
    fn render_at_compact_does_not_panic() {
        render_at(70, 24, 3);
    }
    #[test]
    fn render_at_tiny_does_not_panic() {
        render_at(40, 16, 3);
    }
    #[test]
    fn render_at_pathological_dimensions_does_not_panic() {
        // Smaller than the figlet — verifying we never overflow.
        render_at(30, 10, 1);
        render_at(20, 8, 1);
    }
}
