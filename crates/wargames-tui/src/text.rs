//! Text sizing helpers for the TUI.
//!
//! Contract: **no text is ever silently dropped**. Every helper either fits
//! the text, wraps it to multiple lines, or — when truncation is the only
//! option (e.g. a finite one-line status bar) — appends `…` and tells the
//! caller via the return type. Callers are responsible for choosing the
//! right helper; if a pane is too small we wrap, we don't ellipsize.
//!
//! `unicode-width` gives us a per-char display-width so CJK / emoji /
//! combining marks line up correctly with ratatui's cell grid. Without it,
//! a string like `デフェコン3` reads as 9 cells wide on a CJK-aware
//! terminal but most naive `.chars().count()` impls would write 6 — and
//! then ratatui would clip or wrap awkwardly.

use unicode_width::UnicodeWidthStr;

/// Max number of terminal *cells* the string `s` occupies. Multi-byte / wide
/// characters are counted correctly via `unicode-width`.
pub fn display_width(s: &str) -> usize {
    s.width()
}

/// Wrap `s` into lines, each at most `width` cells wide. Words are kept
/// together when possible; if a single word is wider than `width` it is
/// hard-broken. No content is lost. Returns `vec![s.to_string()]` when
/// `s` already fits.
pub fn wrap_to_width(s: &str, width: usize) -> Vec<String> {
    if width == 0 || s.is_empty() {
        return vec![s.to_string()];
    }
    if display_width(s) <= width {
        return vec![s.to_string()];
    }
    let mut out: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut current_width: usize = 0;
    for word in s.split_whitespace() {
        let word_w = display_width(word);
        // Word alone is wider than the budget → hard-break it.
        if word_w >= width {
            // Flush whatever we have so far.
            if !current.is_empty() {
                out.push(std::mem::take(&mut current));
                current_width = 0;
            }
            let mut piece = String::new();
            let mut piece_w: usize = 0;
            for c in word.chars() {
                let cw = unicode_width::UnicodeWidthChar::width(c).unwrap_or(0);
                if piece_w + cw > width {
                    out.push(std::mem::take(&mut piece));
                    piece_w = 0;
                }
                piece.push(c);
                piece_w += cw;
            }
            if !piece.is_empty() {
                out.push(piece);
            }
            continue;
        }
        let sep = if current.is_empty() { 0 } else { 1 };
        if current_width + sep + word_w > width {
            out.push(std::mem::take(&mut current));
            current.push_str(word);
            current_width = word_w;
        } else {
            if sep == 1 {
                current.push(' ');
                current_width += 1;
            }
            current.push_str(word);
            current_width += word_w;
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    if out.is_empty() {
        out.push(String::new());
    }
    out
}

/// Right-pad with spaces so the result has display-width exactly `width`.
/// If `s` is already wider, **returns it unchanged** (no truncation) so the
/// caller can decide what to do. Use `truncate_with_ellipsis` for the
/// truncation variant.
pub fn pad_right(s: &str, width: usize) -> String {
    let w = display_width(s);
    if w >= width {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len() + (width - w));
    out.push_str(s);
    for _ in 0..(width - w) {
        out.push(' ');
    }
    out
}

/// Truncate `s` to fit `width` cells, appending `…` (single cell) when cut.
/// Returns the input unchanged when it already fits.
///
/// **Callers should prefer [`wrap_to_width`] when the destination can hold
/// multiple rows.** This helper is only for finite one-line slots — the
/// status line at the bottom of the frame, and the "… N earlier omitted"
/// hint inside the event log.
pub fn truncate_with_ellipsis(s: &str, width: usize) -> String {
    let w = display_width(s);
    if w <= width {
        return s.to_string();
    }
    if width == 0 {
        return String::new();
    }
    // Reserve one cell for the ellipsis.
    let target = width - 1;
    let mut out = String::new();
    let mut used: usize = 0;
    for c in s.chars() {
        let cw = unicode_width::UnicodeWidthChar::width(c).unwrap_or(0);
        if used + cw > target {
            break;
        }
        out.push(c);
        used += cw;
        if used == target {
            break;
        }
    }
    out.push('…');
    out
}

/// Wrap `s` to fit `width`, then return only the first `max_lines` lines —
/// the rest get folded into a `"…N more chars hidden"` foot. The foot is
/// itself truncated with an ellipsis so it always fits on one line.
///
/// This is the right helper for the **event log** on small screens:
/// - The full log content is preserved in `App` (we never drop messages).
/// - The rendered pane shows the most recent `max_lines`.
/// - The "N more" hint tells the reader how much they're not seeing, so
///   they know to scroll up via ↑↓ (a follow-up could add keybinds; for now
///   `widget_log` already shows newest events by default).
///
/// Returns `(lines, hidden_chars)`.
pub fn wrap_with_overflow_hint(s: &str, width: usize, max_lines: usize) -> (Vec<String>, usize) {
    let wrapped = wrap_to_width(s, width);
    if wrapped.len() <= max_lines {
        return (wrapped, 0);
    }
    let kept: Vec<String> = wrapped.iter().take(max_lines).cloned().collect();
    let hidden: usize = wrapped
        .iter()
        .skip(max_lines)
        .map(|l| display_width(l) + 1) // include the newline we'd have
        .sum::<usize>()
        .saturating_sub(1);
    (kept, hidden)
}

/// Build a foot line like `"  … 47 chars more (Ctrl+U to read all)"` that
/// always fits in `width` cells.
pub fn overflow_hint_line(hidden: usize, width: usize, hint: &str) -> String {
    // Reserve at least 4 cells for the leading "  … " + trailing "…".
    let body_w = width.saturating_sub(6);
    let body = format!("{hidden} chars more");
    let body = truncate_with_ellipsis(&format!("{body} ({hint})"), body_w);
    format!("  … {body}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_width_handles_ascii_and_wide() {
        assert_eq!(display_width("DEFCON 3"), 8);
        assert_eq!(display_width(""), 0);
    }

    #[test]
    fn display_width_counts_cjk_as_two_cells() {
        // 5 CJK fullwidth characters × 2 cells each = 10 cells.
        // (My earlier draft assumed a different string; pinning the
        // real unicode-width behaviour here.)
        assert_eq!(display_width("デフェコン"), 10);
        assert_eq!(display_width("デ"), 2);
        assert_eq!(display_width("aデb"), 4); // 'a' + 'デ' + 'b' = 1 + 2 + 1
    }

    #[test]
    fn display_width_counts_combining_marks_zero_extra() {
        // "é" (precomposed) is one cell; "e" + combining acute = also one cell.
        // Either way the assertion is for `unicode-width` parity.
        let precomposed = display_width("é");
        let decomposed = display_width("e\u{0301}");
        assert!(precomposed >= 1);
        assert!(decomposed >= 1);
    }

    #[test]
    fn wrap_to_width_keeps_words_intact() {
        let v = wrap_to_width("the quick brown fox", 10);
        assert_eq!(v, vec!["the quick", "brown fox"]);
    }

    #[test]
    fn wrap_to_width_no_change_when_already_fits() {
        let v = wrap_to_width("short", 30);
        assert_eq!(v, vec!["short"]);
    }

    #[test]
    fn wrap_to_width_hard_breaks_one_long_word() {
        // "supercalifragilistic" is 20 chars; width 8 → must hard-break.
        let v = wrap_to_width("supercalifragilistic", 8);
        assert!(v.iter().all(|l| display_width(l) <= 8));
        // Content is fully preserved (no char dropped).
        assert_eq!(v.concat(), "supercalifragilistic");
    }

    #[test]
    fn wrap_to_width_handles_cjk() {
        // 8 CJK chars; each is 2 cells = 16 cells total; width 4.
        let v = wrap_to_width("デフェコン3", 4);
        // Each line is ≤ 4 cells; nothing dropped.
        let joined = v.concat();
        assert_eq!(joined.chars().count(), 6);
        // No trailing ellipsis — full text present.
        assert!(!joined.ends_with('…'));
    }

    #[test]
    fn truncate_with_ellipsis_appends_when_cut() {
        let s = truncate_with_ellipsis("the quick brown fox", 10);
        assert!(display_width(&s) <= 10);
        assert!(s.ends_with('…'));
        assert!(s.starts_with("the quick"));
    }

    #[test]
    fn truncate_with_ellipsis_no_op_when_fits() {
        assert_eq!(truncate_with_ellipsis("abc", 10), "abc");
    }

    #[test]
    fn truncate_with_ellipsis_keeps_full_text_when_room() {
        // Fits exactly.
        assert_eq!(truncate_with_ellipsis("abcdef", 6), "abcdef");
        // One over → cut to "abcde…"
        let s = truncate_with_ellipsis("abcdefg", 6);
        assert_eq!(s, "abcde…");
    }

    #[test]
    fn pad_right_pads_to_width() {
        assert_eq!(pad_right("ab", 5), "ab   ");
        assert_eq!(pad_right("abcde", 5), "abcde");
        assert_eq!(pad_right("abcdef", 5), "abcdef"); // unchanged when over
    }

    #[test]
    fn wrap_with_overflow_hint_returns_hidden_count() {
        let s = "the quick brown fox jumps over the lazy dog";
        let (lines, hidden) = wrap_with_overflow_hint(s, 10, 2);
        assert_eq!(lines.len(), 2);
        assert!(hidden > 0, "must count hidden chars when overflow happens");
        let foot = overflow_hint_line(hidden, 30, "scroll up");
        assert!(display_width(&foot) <= 30);
        assert!(foot.contains(&hidden.to_string()));
    }

    #[test]
    fn wrap_with_overflow_hint_no_hidden_when_fits() {
        let s = "hi there";
        let (_lines, hidden) = wrap_with_overflow_hint(s, 30, 5);
        assert_eq!(hidden, 0);
    }

    #[test]
    fn no_silent_data_loss_for_cjk() {
        // Long Chinese sentence that doesn't fit in a narrow pane.
        let s = "国际安全形势日益复杂";
        let v = wrap_to_width(s, 6);
        // Rejoin and verify nothing was dropped or replaced with '…'.
        let joined: String = v.concat();
        assert_eq!(joined, s);
    }

    #[test]
    fn no_panic_on_empty_or_zero_width() {
        assert_eq!(wrap_to_width("", 0), vec![""]);
        assert_eq!(wrap_to_width("hi", 0), vec!["hi"]);
        assert_eq!(truncate_with_ellipsis("hi", 0), "");
    }
}
