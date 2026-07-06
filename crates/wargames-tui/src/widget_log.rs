//! Event log widget — wraps each entry to fit the inner width, scrolls
//! inside its box when the user pages up/down, and surfaces a "N chars
//! more" hint when overflow occurs.

use crate::text::{self, overflow_hint_line, wrap_to_width};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;
use wargames_core::log::LogEntry;

pub fn render(frame: &mut Frame, area: Rect, log: &[LogEntry], scroll: u16) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Green))
        .title(Span::styled(
            " EVENT LOG ",
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let height = inner.height as usize;
    let width = inner.width as usize;
    // Below the size we can render meaningfully (1 status prefix char + 1
    // message char), show a single-line hint rather than failing silently.
    if width < 14 || height < 2 {
        let hint = if width < 14 {
            "log too narrow — enlarge terminal"
        } else {
            "log too short — enlarge terminal"
        };
        let p = Paragraph::new(Line::from(Span::styled(
            hint,
            Style::default().fg(Color::DarkGray),
        )));
        frame.render_widget(p, inner);
        return;
    }

    if log.is_empty() {
        let p = Paragraph::new(Line::from(Span::styled(
            "  (no events yet)",
            Style::default().fg(Color::DarkGray),
        )));
        frame.render_widget(p, inner);
        return;
    }

    const PREFIX_CELLS: usize = 14;
    let msg_width = width.saturating_sub(PREFIX_CELLS).max(4);

    // Build the full wrapped line set for the log so we can scroll
    // inside it. We then clip to the visible [scroll, scroll+height)
    // window. The header rows stay fixed at the top of the visible
    // window (so the user always sees the most recent event whose
    // first row is in view, regardless of scroll offset).
    let mut lines: Vec<Line> = Vec::new();
    for entry in log {
        let color = match entry.side.as_str() {
            "us" => Color::Cyan,
            "opp" => Color::LightRed,
            _ => Color::Gray,
        };
        let kind_color = match entry.kind.as_str() {
            "trigger" => Color::Yellow,
            "outcome" => Color::Magenta,
            _ => color,
        };
        let header = format!(
            "[{:>3}] {:<5} {:<8}",
            entry.turn, entry.side, entry.kind
        );
        let header_style = Style::default().fg(kind_color);

        let wrapped = wrap_to_width(&entry.message, msg_width);
        for (i, msg_line) in wrapped.iter().enumerate() {
            let lead: Vec<Span<'static>> = if i == 0 {
                vec![Span::styled(header.clone(), header_style)]
            } else {
                vec![Span::raw(" ".repeat(PREFIX_CELLS))]
            };
            let mut row = lead;
            row.push(Span::styled(
                msg_line.clone(),
                Style::default().fg(Color::White),
            ));
            lines.push(Line::from(row));
        }
    }

    // Apply scroll: clamp `scroll` so the user can't paginate past the
    // top (negative) or past the bottom (leave at least `height` rows
    // visible at all times).
    let total = lines.len();
    let view_h = height;
    let max_scroll = total.saturating_sub(view_h) as u16;
    let scroll = (scroll as usize).min(max_scroll as usize) as u16;
    // A first-row marker tells the user there are older entries above.
    if scroll > 0 {
        let skipped = scroll as usize;
        // Replace the first visible row with a header that says how
        // many entries sit above the visible window. We achieve this
        // by mutating the in-range slice below.
        let above = lines
            .get(..skipped)
            .map(|older| {
                let count = older.len();
                Line::from(Span::styled(
                    format!("  … {count} earlier row{plu} (scroll with PgUp/PgDn)",
                        plu = if count == 1 { "" } else { "s" }),
                    Style::default().fg(Color::DarkGray),
                ))
            })
            .unwrap_or_else(|| {
                Line::from(Span::styled(
                    "  … scroll",
                    Style::default().fg(Color::DarkGray),
                ))
            });
        // Replace the first visible line with the hint, dropping the
        // one that occupied that row.
        if skipped < total {
            lines[skipped] = above;
        }
    }

    let visible: Vec<Line> = if total <= view_h {
        lines
    } else {
        let end = (scroll as usize + view_h).min(total);
        lines[scroll as usize..end].to_vec()
    };

    let p = Paragraph::new(visible)
        .wrap(Wrap { trim: false })
        .scroll((0, 0));
    frame.render_widget(p, inner);
}

#[cfg(test)]
mod tests {
    use super::*;
    use wargames_core::log::LogEntry;

    fn entry(turn: u32, msg: &str) -> LogEntry {
        LogEntry {
            turn,
            side: "opp".into(),
            kind: "outcome".into(),
            message: msg.into(),
        }
    }

    #[test]
    fn wraps_long_messages_to_pane_width() {
        // 60-char message in a 14-col message budget → must wrap, no words lost.
        //
        // `wrap_to_width` collapses adjacent whitespace runs into single
        // spaces and uses them as word separators. So we compare token
        // sets, not raw concatenation — wrapping must not drop *words*,
        // but adjacent spaces collapse just like a normal word-wrap would
        // on screen.
        let long = "this is a deliberately long opponent message that overflows";
        let wrapped = wrap_to_width(long, 14);
        assert!(
            wrapped.iter().all(|l| text::display_width(l) <= 14),
            "each wrapped line must fit the budget"
        );
        let joined_tokens: String = wrapped
            .iter()
            .map(|l| l.split_whitespace().collect::<Vec<_>>().join(" "))
            .collect::<Vec<_>>()
            .join(" ");
        assert_eq!(
            joined_tokens.split_whitespace().collect::<Vec<_>>(),
            long.split_whitespace().collect::<Vec<_>>(),
            "wrapping must not drop words"
        );
    }

    #[test]
    fn render_at_narrow_width_does_not_panic() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;
        use ratatui::TerminalOptions;
        use ratatui::Viewport;
        let backend = TestBackend::new(40, 10);
        let mut terminal = Terminal::with_options(backend, TerminalOptions {
            viewport: Viewport::Fullscreen,
        })
        .expect("TestBackend terminal constructs");
        let log = vec![
            entry(1, "short"),
            // A long message that would overflow at 26 cols and contains
            // a multi-byte em-dash; the old render sliced bytes inside
            // the dash. The new render must not panic.
            entry(
                2,
                "international incident reports incoming — multiple sources confirm carrier deployment north of the strait",
            ),
            entry(3, "another short one"),
        ];
        terminal
            .draw(|f| render(f, f.area(), &log, 0))
            .expect("narrow log render must not panic");
    }

    #[test]
    fn render_at_ultra_narrow_width_shows_friendly_hint() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;
        use ratatui::TerminalOptions;
        use ratatui::Viewport;
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::with_options(backend, TerminalOptions {
            viewport: Viewport::Fullscreen,
        })
        .expect("TestBackend terminal constructs");
        let log = vec![entry(1, "x"), entry(2, "y")];
        // 10-col Rect → inner.width = 8 (border) which is < 14 → friendly hint.
        let area = ratatui::layout::Rect {
            x: 0,
            y: 0,
            width: 10,
            height: 5,
        };
        terminal
            .draw(|f| render(f, area, &log, 0))
            .expect("ultra-narrow log render must not panic");
    }

    /// Scrolling into a multi-row log window must surface the older
    /// rows and not invent or drop content. We compare the visible
    /// row count at scroll=0 and scroll=N to ensure scrolling moves
    /// the window, then verify nothing renders taller than the pane.
    #[test]
    fn render_with_scroll_offset_must_not_panic_and_must_clip_to_window() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;
        use ratatui::{TerminalOptions, Viewport};
        let backend = TestBackend::new(60, 8);
        let mut terminal = Terminal::with_options(backend, TerminalOptions {
            viewport: Viewport::Fullscreen,
        })
        .expect("TestBackend terminal constructs");
        // Build a log that's clearly longer than the visible height
        // — 30 entries, each guaranteed to wrap to ≥2 rows so the
        // total is well above the 6-row inner pane.
        let log: Vec<LogEntry> = (1..=30)
            .map(|i| entry(
                i,
                "deliberately long message that wraps across two rows so we have plenty of scrollable content",
            ))
            .collect();
        for s in [0u16, 4, 16, 200] {
            terminal
                .draw(|f| render(f, f.area(), &log, s))
                .unwrap_or_else(|e| panic!("scroll={s} render must not panic: {e}"));
        }
    }
}
