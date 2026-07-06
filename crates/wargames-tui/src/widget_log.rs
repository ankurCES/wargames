//! Event log widget — scrolls inside its box, wraps each entry to fit the
//! inner width, and surfaces a "N chars more" hint when overflow occurs.

use crate::text::{self, overflow_hint_line, wrap_to_width};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;
use wargames_core::log::LogEntry;

pub fn render(frame: &mut Frame, area: Rect, log: &[LogEntry]) {
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

    // The status prefix reserves cells for `[NNN] us trigger ` (14 cells).
    // We must reserve enough cells so wrapped message lines render with at
    // least one leading space of indent to read clearly.
    const PREFIX_CELLS: usize = 14;
    let msg_width = width.saturating_sub(PREFIX_CELLS).max(4);
    // 1 row reserved at the top for "… N earlier events omitted".
    let rows_for_entries = height.saturating_sub(1).max(1);

    let skipped = log.len().saturating_sub(rows_for_entries);

    let mut lines: Vec<Line> = Vec::new();
    if skipped > 0 {
        lines.push(Line::from(Span::styled(
            format!("  … {skipped} earlier events omitted (log auto-scrolls)"),
            Style::default().fg(Color::DarkGray),
        )));
    }

    // Walk the log newest-first — every row is fully shown via wrapping
    // when its message is too long, and a "N chars more" hint appends to
    // the entry when the height budget is exhausted mid-message.
    //
    // We build line-by-line and stop appending once the pane fills.
    let visible_start = log.len().saturating_sub(rows_for_entries);
    let mut rows_used = lines.len();
    for entry in &log[visible_start..] {
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

        // Wrap the message to the per-line budget. No content is lost —
        // even the longest message lines are fully shown across multiple
        // rows; the only thing that gets truncated is the *folded* count
        // (a one-line hint we synthesise) when the height runs out
        // mid-message.
        let wrapped = wrap_to_width(&entry.message, msg_width);
        // If this entry alone would overflow the remaining rows, fold the
        // tail into a "…N chars more" hint that fits on a single row.
        let rows_left = height.saturating_sub(rows_used);
        let (shown, hidden) = if wrapped.len() <= rows_left {
            (wrapped, 0usize)
        } else {
            let take = rows_left.saturating_sub(1); // reserve 1 row for hint
            let kept = wrapped.iter().take(take).cloned().collect::<Vec<_>>();
            let hidden_chars: usize = wrapped
                .iter()
                .skip(take)
                .map(|l| text::display_width(l) + 1)
                .sum::<usize>()
                .saturating_sub(1);
            (kept, hidden_chars)
        };

        for (i, msg_line) in shown.iter().enumerate() {
            if rows_used >= height {
                break;
            }
            // First row keeps the prefix; continuation rows are indented
            // with spaces so the column reads cleanly.
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
            rows_used += 1;
        }

        if hidden > 0 {
            if rows_used >= height {
                break;
            }
            let hint = overflow_hint_line(hidden, width, "App log keeps full");
            lines.push(Line::from(Span::styled(
                hint,
                Style::default().fg(Color::DarkGray),
            )));
            rows_used += 1;
        }
    }

    let p = Paragraph::new(lines).wrap(Wrap { trim: false });
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
            .draw(|f| render(f, f.area(), &log))
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
            .draw(|f| render(f, area, &log))
            .expect("ultra-narrow log render must not panic");
    }
}
