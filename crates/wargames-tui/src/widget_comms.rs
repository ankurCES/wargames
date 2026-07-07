//! Comms panel — multi-language side-channel messages with
//! priority coloring and language-tagged prefix arrows.
//!
//! Pure renderer: it takes a slice of `LogEntry` and a height
//! hint, builds styled lines, and hands them to `Paragraph`.
//! RTL scripts get a leading `←` so the player can tell the
//! direction of flow before reading.

use crate::theme;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;
use wargames_core::log::LogEntry;
use wargames_core::Language;

/// Render the comms panel into `area`. Shows the most recent
/// `max_lines` entries (latest at the bottom). Non-comm log
/// entries are skipped — this widget is strictly for `kind == "comm"`.
pub fn render(
    frame: &mut Frame,
    area: Rect,
    log: &[LogEntry],
    tick: u64,
    max_lines: usize,
) {
    let theme = theme::current();
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border))
        .title(Span::styled(
            " COMMS ",
            Style::default().fg(theme.title).bold(),
        ));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.width < 6 || inner.height < 2 {
        return;
    }

    let mut comms: Vec<&LogEntry> = log.iter().filter(|e| e.kind == "comm").collect();
    // Tail to the last `max_lines`.
    if comms.len() > max_lines {
        comms = comms.split_off(comms.len() - max_lines);
    }

    let mut lines: Vec<Line<'static>> = Vec::new();
    if comms.is_empty() {
        lines.push(Line::from(Span::styled(
            "  (channel quiet)",
            Style::default().fg(theme.radar_ghost),
        )));
    } else {
        for entry in comms {
            lines.push(line_for(entry, &theme, tick));
        }
    }

    let p = Paragraph::new(lines).wrap(Wrap { trim: false });
    frame.render_widget(p, inner);
}

fn line_for(entry: &LogEntry, theme: &theme::Theme, tick: u64) -> Line<'static> {
    // Direction arrow: ← for RTL (Arabic, Hebrew), → for LTR.
    let arrow = if entry.language.is_rtl() { "← " } else { "→ " };

    // Side color: "us" = our color, "opp" = red, anything else = grey.
    let side_style = match entry.side.as_str() {
        "us" => Style::default().fg(theme.radar_us),
        "opp" => Style::default().fg(Color::LightRed),
        _ => Style::default().fg(theme.status_text),
    };

    // Priority is approximated by language: non-English dialects
    // are higher-priority in the WOPR scenario, plus Cyrillic /
    // CJK get the "foreign" yellow tag.
    let lang_style = match entry.language {
        Language::English => Style::default().fg(theme.status_text),
        Language::Russian => Style::default().fg(Color::Red),
        Language::Mandarin => Style::default().fg(Color::Yellow),
        Language::Korean => Style::default().fg(Color::Yellow),
        Language::Arabic => Style::default().fg(Color::Magenta),
        Language::Hebrew => Style::default().fg(Color::Magenta),
    };

    // Add a subtle blink to opp-side messages every ~30 ticks so
    // the user notices them in their peripheral vision.
    let mut side_with_blink = side_style;
    if entry.side == "opp" && (tick / 30) % 2 == 0 {
        side_with_blink = side_with_blink.add_modifier(Modifier::SLOW_BLINK);
    }

    vec![
        Span::styled(format!("{arrow}[{}] ", entry.language.as_str()), lang_style),
        Span::styled(entry.message.clone(), side_with_blink),
    ]
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::{Terminal, TerminalOptions, Viewport};
    use wargames_core::log::LogEntry;

    fn comm(side: &str, msg: &str) -> LogEntry {
        LogEntry::comm(1, side, msg)
    }

    fn rtl_comm(side: &str, msg: &str) -> LogEntry {
        LogEntry::comm_with_lang(1, side, Language::Arabic, msg)
    }

    #[test]
    fn empty_log_renders_placeholder() {
        let backend = TestBackend::new(60, 8);
        let mut terminal = Terminal::with_options(
            backend,
            TerminalOptions { viewport: Viewport::Fullscreen },
        )
        .expect("terminal");
        terminal
            .draw(|f| render(f, f.area(), &[], 0, 10))
            .expect("empty render must not panic");
        let buf = terminal.backend().buffer().clone();
        let mut s = String::new();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                s.push_str(buf[(x, y)].symbol());
            }
        }
        assert!(s.contains("channel quiet"), "expected placeholder line: {s}");
    }

    #[test]
    fn opp_comm_renders_with_arrow() {
        let backend = TestBackend::new(60, 8);
        let mut terminal = Terminal::with_options(
            backend,
            TerminalOptions { viewport: Viewport::Fullscreen },
        )
        .expect("terminal");
        let log = vec![comm("opp", "We are watching."), comm("us", "Acknowledged.")];
        terminal
            .draw(|f| render(f, f.area(), &log, 0, 10))
            .expect("render");
        let buf = terminal.backend().buffer().clone();
        let mut s = String::new();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                s.push_str(buf[(x, y)].symbol());
            }
        }
        assert!(s.contains("→ "), "LTR arrow missing: {s}");
        assert!(s.contains("We are watching."), "opp message missing: {s}");
        assert!(s.contains("Acknowledged."), "us message missing: {s}");
    }

    #[test]
    fn rtl_comm_renders_left_arrow() {
        let backend = TestBackend::new(60, 8);
        let mut terminal = Terminal::with_options(
            backend,
            TerminalOptions { viewport: Viewport::Fullscreen },
        )
        .expect("terminal");
        let log = vec![rtl_comm("opp", "rtl payload")];
        terminal
            .draw(|f| render(f, f.area(), &log, 0, 10))
            .expect("render");
        let buf = terminal.backend().buffer().clone();
        let mut s = String::new();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                s.push_str(buf[(x, y)].symbol());
            }
        }
        assert!(s.contains("← "), "RTL arrow missing: {s}");
    }

    #[test]
    fn non_comm_entries_are_skipped() {
        let backend = TestBackend::new(60, 8);
        let mut terminal = Terminal::with_options(
            backend,
            TerminalOptions { viewport: Viewport::Fullscreen },
        )
        .expect("terminal");
        // Only "comm" entries should be visible. Triggers/outcomes/etc
        // must not appear in the panel.
        let log = vec![
            LogEntry::trigger(1, "TRIGGER PAYLOAD"),
            LogEntry::outcome(1, "OUTCOME PAYLOAD"),
            comm("us", "visible"),
        ];
        terminal
            .draw(|f| render(f, f.area(), &log, 0, 10))
            .expect("render");
        let buf = terminal.backend().buffer().clone();
        let mut s = String::new();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                s.push_str(buf[(x, y)].symbol());
            }
        }
        assert!(!s.contains("TRIGGER PAYLOAD"), "trigger leaked into comms: {s}");
        assert!(!s.contains("OUTCOME PAYLOAD"), "outcome leaked into comms: {s}");
        assert!(s.contains("visible"), "comm message missing: {s}");
    }

    #[test]
    fn respects_max_lines_cap() {
        let backend = TestBackend::new(60, 8);
        let mut terminal = Terminal::with_options(
            backend,
            TerminalOptions { viewport: Viewport::Fullscreen },
        )
        .expect("terminal");
        let log = vec![
            comm("us", "FIRST"),
            comm("us", "SECOND"),
            comm("us", "THIRD"),
            comm("us", "FOURTH"),
        ];
        terminal
            .draw(|f| render(f, f.area(), &log, 0, 2))
            .expect("render");
        let buf = terminal.backend().buffer().clone();
        let mut s = String::new();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                s.push_str(buf[(x, y)].symbol());
            }
        }
        // Only the most recent 2 (THIRD + FOURTH) should appear.
        assert!(s.contains("THIRD"));
        assert!(s.contains("FOURTH"));
        assert!(!s.contains("FIRST"));
        assert!(!s.contains("SECOND"));
    }

    #[test]
    fn render_handles_sub_minimum_area() {
        let backend = TestBackend::new(8, 3);
        let mut terminal = Terminal::with_options(
            backend,
            TerminalOptions { viewport: Viewport::Fullscreen },
        )
        .expect("terminal");
        let log = vec![comm("us", "hi")];
        terminal
            .draw(|f| render(f, f.area(), &log, 0, 5))
            .expect("render must early-return cleanly");
    }
}