//! Comms panel — multi-language side-channel messages with
//! priority coloring and language-tagged prefix arrows.
//!
//! Each rendered comm optionally carries a translation line beneath
//! it, sourced from the bundled `TRANSLATIONS` table. Translations
//! are a deliberate offline fallback: the LLM streaming path still
//! produces English-by-default, but the canned comms that ship with
//! the picker/scenarios are pre-translated so the panel is
//! informative even without an API key.
//!
//! Pure renderer: it takes a slice of `LogEntry` and a height
//! hint, builds styled lines, and hands them to `Paragraph`.
//! RTL scripts get a leading `←` so the player can tell the
//! direction of flow before reading.

// ─── Translation table ──────────────────────────────────────────
//
// Each entry is `(canonical_english, language, translated_text)`.
// Lookup is by exact match on the *source* English message — that
// keeps the table deterministic and easy to extend without a fuzzy
// matcher. Missing keys fall through to `None` and the renderer
// omits the translation line entirely.
//
// Curated by hand to keep the table small and high-quality; new
// canned comms should be added here as they're introduced so the
// panel stays consistent across runs. Live (LLM-streamed) comms
// are *not* in this table — they render the original English only
// until the streaming buffer is closed.
const TRANSLATIONS: &[(&str, Language, &str)] = &[
    // ── Russian ───────────────────────────────────────────────
    (
        "We are watching.",
        Language::Russian,
        "Мы наблюдаем.",
    ),
    (
        "All forces standing by.",
        Language::Russian,
        "Все силы в готовности.",
    ),
    (
        "Missiles armed.",
        Language::Russian,
        "Ракеты приведены в готовность.",
    ),
    (
        "Acknowledged.",
        Language::Russian,
        "Принято.",
    ),
    (
        "Launch detected.",
        Language::Russian,
        "Обнаружен запуск.",
    ),
    (
        "Stand down.",
        Language::Russian,
        "Отбой.",
    ),
    // ── Mandarin ──────────────────────────────────────────────
    (
        "We are watching.",
        Language::Mandarin,
        "我们正在观察。",
    ),
    (
        "All forces standing by.",
        Language::Mandarin,
        "所有部队待命。",
    ),
    (
        "Missiles armed.",
        Language::Mandarin,
        "导弹已就绪。",
    ),
    (
        "Acknowledged.",
        Language::Mandarin,
        "已收到。",
    ),
    (
        "Launch detected.",
        Language::Mandarin,
        "检测到发射。",
    ),
    (
        "Stand down.",
        Language::Mandarin,
        "解除戒备。",
    ),
    // ── Korean ────────────────────────────────────────────────
    (
        "We are watching.",
        Language::Korean,
        "우리는 관찰하고 있다.",
    ),
    (
        "Acknowledged.",
        Language::Korean,
        "수신 완료.",
    ),
    (
        "Missiles armed.",
        Language::Korean,
        "미사일 준비 완료.",
    ),
    (
        "Launch detected.",
        Language::Korean,
        "발사 감지됨.",
    ),
    // ── Arabic (RTL) ──────────────────────────────────────────
    (
        "We are watching.",
        Language::Arabic,
        "نحن نراقب.",
    ),
    (
        "All forces standing by.",
        Language::Arabic,
        "جميع القوات في حالة استعداد.",
    ),
    (
        "Missiles armed.",
        Language::Arabic,
        "الصواريخ جاهزة.",
    ),
    (
        "Acknowledged.",
        Language::Arabic,
        "تم الاستلام.",
    ),
    (
        "Launch detected.",
        Language::Arabic,
        "تم اكتشاف إطلاق.",
    ),
    // ── Hebrew (RTL) ──────────────────────────────────────────
    (
        "We are watching.",
        Language::Hebrew,
        "אנחנו צופים.",
    ),
    (
        "All forces standing by.",
        Language::Hebrew,
        "כל הכוחות בכוננות.",
    ),
    (
        "Missiles armed.",
        Language::Hebrew,
        "הטילים מוכנים.",
    ),
    (
        "Acknowledged.",
        Language::Hebrew,
        "אושר.",
    ),
    (
        "Launch detected.",
        Language::Hebrew,
        "זוהה שיגור.",
    ),
];

/// Look up a translation for `english_source` in `target`. Returns
/// `None` when no translation is bundled — the renderer then drops
/// the translation line entirely (silent fallback, no panic).
///
/// Exact-match only on the English source. Leading/trailing
/// whitespace is trimmed so callers don't have to be pedantic
/// about `LogEntry::comm("us", "We are watching. ")` trailing
/// space, but we don't otherwise normalize the source — that's
/// the caller's responsibility.
pub fn translate(english_source: &str, target: Language) -> Option<&'static str> {
    let trimmed = english_source.trim();
    TRANSLATIONS
        .iter()
        .find(|(src, lang, _)| *src == trimmed && *lang == target)
        .map(|(_, _, translated)| *translated)
}

use crate::theme;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
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
            // Beneath each comm, render a translated copy in every
            // language for which we have a bundled translation.
            // Each translation gets its own row so the panel stays
            // scannable when the user knows which language they're
            // looking for. Empty when nothing matches.
            for translated_line in translation_lines_for(entry, &theme) {
                lines.push(translated_line);
            }
        }
    }

    // Each Line is already one row; we deliberately do NOT use
    // Wrap here because ratatui's Wrap algorithm inserts single
    // spaces between consecutive CJK characters when a line is
    // narrower than the content width — that visually corrupts
    // Korean / Mandarin / Japanese translation rows. Our translation
    // strings are short enough to fit any realistic panel width
    // and the canonical English line is capped by the comm message.
    frame.render_widget(Paragraph::new(lines), inner);
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

/// Build the per-language translation rows shown beneath a comm.
/// Each row is a single `Line` with:
///   - a dim language tag (e.g. `[ru]`)
///   - the translated text in the language's own direction arrow
///   - `→` for LTR scripts (ru, zh, ko)
///   - `←` for RTL scripts (ar, he)
///
/// Translation rows are skipped for languages whose bundled text
/// is missing — silent fallback, never an error or empty placeholder.
/// The original-language row is *not* re-emitted here; that's the
/// caller's job (`line_for`).
fn translation_lines_for(entry: &LogEntry, theme: &theme::Theme) -> Vec<Line<'static>> {
    let mut out = Vec::new();
    // Iterate every supported language except the source language —
    // re-emitting the source would just be visual noise.
    for target in TRANSLATION_TARGETS {
        if *target == entry.language {
            continue;
        }
        if let Some(translated) = translate(&entry.message, *target) {
            let arrow = if target.is_rtl() { "← " } else { "→ " };
            let tag_color = match target {
                Language::Russian => Color::Red,
                Language::Mandarin => Color::Yellow,
                Language::Korean => Color::Yellow,
                Language::Arabic => Color::Magenta,
                Language::Hebrew => Color::Magenta,
                Language::English => theme.status_text,
            };
            out.push(Line::from(vec![
                Span::styled("  ", Style::default()),
                Span::styled(
                    format!("{arrow}[{}] ", target.as_str()),
                    Style::default().fg(tag_color),
                ),
                Span::styled(
                    translated.to_string(),
                    Style::default().fg(theme.radar_ghost),
                ),
            ]));
        }
    }
    out
}

/// All languages we render translations for, in canonical order.
/// Source language is filtered at the call site.
const TRANSLATION_TARGETS: &[Language] = &[
    Language::Russian,
    Language::Mandarin,
    Language::Korean,
    Language::Arabic,
    Language::Hebrew,
];

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

    /// Flatten a ratatui TestBackend buffer into a single string.
    /// Ratatui stores wide (width-2) chars like CJK in a single
    /// Cell, with the *next* Cell acting as a continuation that
    /// reads as `" "`. A naive column-by-column sweep reads each
    /// wide char as `"<char> "`, which mangles the CJK output of
    /// our translation rows. Walk the buffer's underlying `content`
    /// slice in cell order and skip the trailing cell of any wide
    /// grapheme using `unicode-width`'s display-width.
    fn buffer_string(buf: &ratatui::buffer::Buffer) -> String {
        use unicode_width::UnicodeWidthChar;
        let mut s = String::new();
        for y in 0..buf.area.height {
            let mut x = 0u16;
            while x < buf.area.width {
                let cell = &buf[(x, y)];
                let symbol = cell.symbol();
                let mut width = 0u16;
                for c in symbol.chars() {
                    width += UnicodeWidthChar::width(c).unwrap_or(0) as u16;
                }
                s.push_str(symbol);
                // Ratatui reserves the *next* cell as a width-0
                // continuation when the current glyph is double-width
                // (e.g. CJK). Advance past it so the inner loop doesn't
                // re-emit its placeholder symbol as a stray space.
                if width > 1 {
                    x += width;
                } else {
                    x += 1;
                }
            }
        }
        s
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
        let s = buffer_string(&buf);
        assert!(s.contains("channel quiet"), "expected placeholder line: {s}");
    }

    #[test]
    fn opp_comm_renders_with_arrow() {
        // Buffer must be tall enough to fit 2 source comms + 5
        // translation rows each (10 rows total) plus borders.
        let backend = TestBackend::new(60, 18);
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
        let s = buffer_string(&buf);
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
        let s = buffer_string(&buf);
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
        let s = buffer_string(&buf);
        assert!(s.contains("TRIGGER PAYLOAD") == false, "trigger leaked into comms: {s}");
        assert!(s.contains("OUTCOME PAYLOAD") == false, "outcome leaked into comms: {s}");
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
        let s = buffer_string(&buf);
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

    // ─── Translation lookup + render tests ────────────────────────

    #[test]
    fn translate_returns_known_russian_string() {
        let got = translate("We are watching.", Language::Russian);
        assert_eq!(got, Some("Мы наблюдаем."));
    }

    #[test]
    fn translate_returns_known_mandarin_string() {
        let got = translate("Missiles armed.", Language::Mandarin);
        assert_eq!(got, Some("导弹已就绪。"));
    }

    #[test]
    fn translate_returns_known_korean_string() {
        let got = translate("Acknowledged.", Language::Korean);
        assert_eq!(got, Some("수신 완료."));
    }

    #[test]
    fn translate_returns_known_arabic_string() {
        let got = translate("Launch detected.", Language::Arabic);
        assert_eq!(got, Some("تم اكتشاف إطلاق."));
    }

    #[test]
    fn translate_returns_known_hebrew_string() {
        let got = translate("Stand down.", Language::Hebrew);
        // No "Stand down." in the Hebrew slice — must be None.
        assert_eq!(got, None);
        // But "All forces standing by." *is* in the Hebrew slice.
        let got = translate("All forces standing by.", Language::Hebrew);
        assert_eq!(got, Some("כל הכוחות בכוננות."));
    }

    #[test]
    fn translate_trims_trailing_whitespace() {
        // Callers don't have to be pedantic about exact spacing.
        let got = translate("  We are watching.  ", Language::Russian);
        assert_eq!(got, Some("Мы наблюдаем."));
    }

    #[test]
    fn translate_returns_none_for_unknown_source() {
        let got = translate("Zalgo rises at dawn.", Language::Russian);
        assert_eq!(got, None);
    }

    #[test]
    fn translate_returns_none_for_unsupported_pair() {
        // "All forces standing by." exists in Russian but not in
        // Korean. The table is sparse — that's the explicit design.
        let got = translate("All forces standing by.", Language::Korean);
        assert_eq!(got, None, "Korean 'All forces' not in table — must be None");
    }

    #[test]
    fn translation_lines_for_skips_source_language() {
        // An English-source comm must NOT emit a redundant English
        // translation line — only the 5 other languages.
        // All 5 non-English targets are in the table for
        // "We are watching.", so we expect 5 translation lines.
        let entry = comm("us", "We are watching.");
        let lines = translation_lines_for(&entry, &theme::current());
        assert_eq!(lines.len(), 5);
        // None of the emitted lines should contain the English
        // source text — they're all translated copies.
        for line in &lines {
            let combined: String = line
                .spans
                .iter()
                .map(|s| s.content.to_string())
                .collect();
            assert!(!combined.contains("english"), "must skip source language: {combined}");
            assert!(!combined.contains("We are watching."));
        }
    }

    #[test]
    fn translation_lines_for_rtl_uses_left_arrow() {
        // Russian source — Arabic translation is RTL and must use ←.
        let entry = LogEntry::comm_with_lang(1, "opp", Language::Russian, "We are watching.");
        let lines = translation_lines_for(&entry, &theme::current());
        let arabic_line = lines
            .iter()
            .find(|l| {
                let s: String = l.spans.iter().map(|sp| sp.content.to_string()).collect();
                s.contains("[arabic]")
            })
            .expect("Arabic translation line must exist");
        let combined: String = arabic_line
            .spans
            .iter()
            .map(|s| s.content.to_string())
            .collect();
        // Translation rows indent with two spaces before the arrow,
        // so the RTL marker is mid-line, not at column 0. Check that
        // the ← appears in this row (anywhere — column 0 indent is
        // cosmetic; the semantic direction is what matters).
        assert!(combined.contains("←"), "RTL line must use ← arrow: {combined}");
        assert!(combined.contains("[arabic]"));
    }

    #[test]
    fn translation_lines_for_unknown_source_emits_nothing() {
        let entry = comm("us", "Zalgo rises at dawn.");
        let lines = translation_lines_for(&entry, &theme::current());
        assert!(
            lines.is_empty(),
            "no translation lines when source is unknown: {lines:?}"
        );
    }

    #[test]
    fn render_panel_shows_translations_beneath_canonical_comm() {
        // Tall enough for 1 canonical + 5 translation rows + borders.
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::with_options(
            backend,
            TerminalOptions { viewport: Viewport::Fullscreen },
        )
        .expect("terminal");
        let log = vec![comm("us", "We are watching.")];
        terminal
            .draw(|f| render(f, f.area(), &log, 0, 10))
            .expect("render");
        let buf = terminal.backend().buffer().clone();
        let s = buffer_string(&buf);
        // The Russian, Chinese, Korean, and Arabic translations
        // must all appear in the rendered buffer.
        assert!(s.contains("Мы наблюдаем."), "Russian translation missing: {s}");
        assert!(s.contains("我们正在观察。"), "Chinese translation missing: {s}");
        assert!(s.contains("우리는 관찰하고 있다."), "Korean translation missing: {s}");
        assert!(s.contains("نحن نراقب."), "Arabic translation missing: {s}");
        // And the language tags must be visible.
        assert!(s.contains("[russian]"));
        assert!(s.contains("[mandarin]"));
        assert!(s.contains("[korean]"));
        assert!(s.contains("[arabic]"));
    }
}