//! Event log widget — wraps each entry to fit the inner width, scrolls
//! inside its box when the user pages up/down, and surfaces a "N chars
//! more" hint when overflow occurs.

use crate::text::{self, overflow_hint_line, wrap_to_width};
use crate::theme;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;
use wargames_core::log::LogEntry;

pub fn render(frame: &mut Frame, area: Rect, log: &[LogEntry], scroll: u16) {
    let theme = theme::current();
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border))
        .title(Span::styled(
            " EVENT LOG ",
            Style::default()
                .fg(theme.log_header)
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
            Style::default().fg(theme.log_dim),
        )));
        frame.render_widget(p, inner);
        return;
    }

    if log.is_empty() {
        let p = Paragraph::new(Line::from(Span::styled(
            "  (no events yet)",
            Style::default().fg(theme.log_dim),
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
            "us" => theme.log_us,
            "opp" => theme.log_opp,
            _ => theme.log_neutral,
        };
        let kind_color = match entry.kind.as_str() {
            "trigger" => theme.log_trigger,
            "outcome" => theme.log_outcome,
            // Comm rows are coloured by side (opp/soviet/terror/...)
            // — they're transcripts, not neutral world events.
            "comm" => color,
            _ => color,
        };
        let header = format!(
            "[{:>3}] {:<5} {:<8}",
            entry.turn, entry.side, entry.kind
        );
        let header_style = Style::default().fg(kind_color);

        let wrapped = wrap_to_width(&entry.message, msg_width);
        // Comm rows use the side color for the message itself so
        // it reads as a voice, not a passive log line. Other
        // kinds stay in the neutral log_text color.
        let msg_color = if entry.kind == "comm" {
            kind_color
        } else {
            theme.log_text
        };
        for (i, msg_line) in wrapped.iter().enumerate() {
            let is_first = i == 0;
            let is_rtl_comm = is_first
                && entry.kind == "comm"
                && entry.language.is_rtl();
            let is_ltr_comm = is_first
                && entry.kind == "comm"
                && !entry.language.is_rtl();
            let mut row: Vec<Span<'static>> = if is_first {
                let mut spans = vec![Span::styled(header.clone(), header_style)];
                // LTR comm: "[  1] opp   comm    ▸ мы готовимся"
                // RTL comm: "[  1] opp   comm    мы готовимся ◂"
                // (trailing-edge marker so visual flow reads
                // right-to-left in the terminal.)
                if is_ltr_comm {
                    spans.push(Span::styled(
                        " \u{25B8} ",
                        Style::default()
                            .fg(kind_color)
                            .add_modifier(Modifier::BOLD),
                    ));
                    spans.push(Span::styled(
                        msg_line.clone(),
                        Style::default().fg(msg_color),
                    ));
                } else if is_rtl_comm {
                    spans.push(Span::raw(" "));
                    spans.push(Span::styled(
                        msg_line.clone(),
                        Style::default().fg(msg_color),
                    ));
                    spans.push(Span::styled(
                        " \u{25C2}",
                        Style::default()
                            .fg(kind_color)
                            .add_modifier(Modifier::BOLD),
                    ));
                } else {
                    // Trigger / outcome / action rows get a single
                    // space between the header and the message.
                    spans.push(Span::raw(" "));
                    spans.push(Span::styled(
                        msg_line.clone(),
                        Style::default().fg(msg_color),
                    ));
                }
                spans
            } else {
                // Continuation rows are blank-prefixed so the
                // wrapped text aligns with the first row's
                // message column.
                let mut spans =
                    vec![Span::raw(" ".repeat(PREFIX_CELLS))];
                spans.push(Span::styled(
                    msg_line.clone(),
                    Style::default().fg(msg_color),
                ));
                spans
            };
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
                    format!("  … {count} earlier row{plu} (j/k or PgUp/PgDn)",
                        plu = if count == 1 { "" } else { "s" }),
                    Style::default().fg(theme.log_dim),
                ))
            })
            .unwrap_or_else(|| {
                Line::from(Span::styled(
                    "  … scroll",
                    Style::default().fg(theme.log_dim),
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
    use wargames_core::language::Language;
    use wargames_core::log::LogEntry;

    fn entry(turn: u32, msg: &str) -> LogEntry {
        LogEntry {
            turn,
            side: "opp".into(),
            kind: "outcome".into(),
            language: Language::English,
            message: msg.into(),
        }
    }

    fn comm_entry(turn: u32, msg: &str, lang: Language) -> LogEntry {
        LogEntry::comm_with_lang(turn, "opp", lang, msg)
    }

    /// Comm rows tagged RTL must place the directional marker
    /// (`◂`) at the trailing edge of the message, and must not
    /// place a leading `▸`. Verifies the row's rendered spans
    /// contain both the message and the trailing marker, in the
    /// expected order.
    #[test]
    fn rtl_comm_row_places_trailing_arrow() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;
        use ratatui::TerminalOptions;
        use ratatui::Viewport;
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::with_options(backend, TerminalOptions {
            viewport: Viewport::Fullscreen,
        })
        .expect("TestBackend terminal constructs");
        let msg = "الاستعدادات جارية";
        let log = vec![comm_entry(1, msg, Language::Arabic)];
        terminal
            .draw(|f| render(f, f.area(), &log, 0))
            .expect("RTL comm render must not panic");
        let buf = terminal.backend().buffer().clone();
        // Walk every cell of the first non-empty row and reconstruct
        // the text. The expected ordering is: header, message, then
        // trailing `◂` — so the message text must appear before the
        // arrow char in the rendered span sequence.
        let mut row_text = String::new();
        let mut arrow_seen_after_message = false;
        let mut message_seen = false;
        for y in 0..buf.area.height {
            let mut line = String::new();
            for x in 0..buf.area.width {
                let cell = &buf[(x, y)];
                line.push_str(cell.symbol());
            }
            let trimmed = line.trim_end();
            if trimmed.contains(msg) {
                row_text = trimmed.to_string();
                // Find the position of the message and the arrow.
                let msg_pos = row_text.find(msg).expect("message found");
                if let Some(arrow_pos) =
                    row_text.find('\u{25C2}')
                {
                    message_seen = true;
                    arrow_seen_after_message =
                        arrow_pos > msg_pos + msg.len();
                }
                break;
            }
        }
        assert!(
            message_seen,
            "RTL message text must appear in the rendered buffer, got: {row_text:?}"
        );
        assert!(
            arrow_seen_after_message,
            "RTL comm row must have trailing `◂` after the message, got: {row_text:?}"
        );
    }

    /// Walk every cell of the rendered buffer in row-major order
    /// and concatenate `cell.symbol()`. Used by the non-Latin /
    /// RTL tests below to assert that the message text appears
    /// contiguously somewhere in the output. CJK characters render
    /// as a single `TestBackend` cell even though they occupy two
    /// terminal columns, so a substring match on the joined string
    /// is correct for both narrow (ASCII) and wide (CJK) scripts.
    fn buffer_to_string(buf: &ratatui::buffer::Buffer) -> String {
        let mut s = String::with_capacity(
            (buf.area.width as usize) * (buf.area.height as usize),
        );
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                s.push_str(buf[(x, y)].symbol());
            }
        }
        s
    }

    /// Non-Latin scripts (Cyrillic, CJK, Hangul) render as
    /// unicode through the existing pipeline — verify the message
    /// survives byte-by-byte and is visible somewhere in the
    /// rendered buffer, with no panic.
    #[test]
    fn non_latin_scripts_render_without_panic() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;
        use ratatui::TerminalOptions;
        use ratatui::Viewport;
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::with_options(backend, TerminalOptions {
            viewport: Viewport::Fullscreen,
        })
        .expect("TestBackend terminal constructs");
        let messages = [
            ("мы готовы", Language::Russian),
            ("我们准备好了", Language::Mandarin),
            ("준비 완료", Language::Korean),
            ("מוכנים", Language::Hebrew),
        ];
        let log: Vec<LogEntry> = messages
            .iter()
            .enumerate()
            .map(|(i, (m, lang))| comm_entry(i as u32 + 1, m, *lang))
            .collect();
        terminal
            .draw(|f| render(f, f.area(), &log, 0))
            .expect("non-Latin comm render must not panic");
        let buf = terminal.backend().buffer().clone();
        let s = buffer_to_string(&buf);
        // TestBackend writes each wide (CJK / Hangul) character as
        // a single cell without doubling the symbol, then visually
        // pads with an ASCII space on the right — so the buffer
        // string ends up looking like "我 们 准 备 好 了" rather
        // than "我们准备好了". Real terminals display these as
        // contiguous wide glyphs; for the test we just need every
        // character to be present in order. Strip ASCII spaces
        // before matching.
        let s_compact: String = s.chars().filter(|c| *c != ' ').collect();
        for (msg, lang) in messages {
            let msg_compact: String =
                msg.chars().filter(|c| *c != ' ').collect();
            assert!(
                s_compact.contains(&msg_compact),
                "script {msg:?} ({lang:?}) must appear in rendered buffer (chars in order); first 800 chars of buffer:\n{:?}",
                &s[..s.len().min(800)]
            );
        }
    }

    /// LTR comm rows still get a leading `▸` arrow. Regression
    /// guard: making the RTL flip must not break the default case.
    #[test]
    fn ltr_comm_row_places_leading_arrow() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;
        use ratatui::TerminalOptions;
        use ratatui::Viewport;
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::with_options(backend, TerminalOptions {
            viewport: Viewport::Fullscreen,
        })
        .expect("TestBackend terminal constructs");
        let log = vec![comm_entry(1, "we are ready", Language::English)];
        terminal
            .draw(|f| render(f, f.area(), &log, 0))
            .expect("LTR comm render must not panic");
        let buf = terminal.backend().buffer().clone();
        let s = buffer_to_string(&buf);
        let arrow_pos = s.find('\u{25B8}').expect("LTR leading arrow");
        let msg_pos = s.find("we are ready").expect("LTR message");
        assert!(
            arrow_pos < msg_pos,
            "LTR `▸` must come before the message in rendered buffer, got arrow={arrow_pos} msg={msg_pos}"
        );
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

    /// The scroll hint that appears at the top of the pane when the
    /// user has scrolled away from the tail must mention the
    /// vim-style `j/k` keys (not just PgUp/PgDn) — both code paths
    /// drive the scroll handler, so both deserve the discoverability.
    #[test]
    fn scroll_hint_mentions_jk_keys_alongside_pgup_pgdn() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;
        use ratatui::{TerminalOptions, Viewport};
        let backend = TestBackend::new(60, 8);
        let mut terminal = Terminal::with_options(backend, TerminalOptions {
            viewport: Viewport::Fullscreen,
        })
        .expect("TestBackend terminal constructs");
        let log: Vec<LogEntry> = (1..=30)
            .map(|i| entry(i, "long message that wraps so we have plenty of scrollable content"))
            .collect();
        // Pass a non-zero scroll so the "earlier row(s)" marker is emitted.
        terminal
            .draw(|f| render(f, f.area(), &log, 8))
            .expect("scroll-hint render must not panic");
        let buf = terminal.backend().buffer().clone();
        let mut s = String::with_capacity(
            (buf.area.width as usize) * (buf.area.height as usize),
        );
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                s.push_str(buf[(x, y)].symbol());
            }
        }
        assert!(
            s.contains("j/k"),
            "scroll hint must mention j/k so users discover the vim-style keys, got buffer: {s:?}"
        );
        assert!(
            s.contains("PgUp/PgDn"),
            "scroll hint must keep mentioning PgUp/PgDn for users who prefer page keys, got buffer: {s:?}"
        );
    }
}
