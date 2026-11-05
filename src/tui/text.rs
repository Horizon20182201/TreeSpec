//! Terminal display helpers. Every string from disk, the LLM or an error
//! message passes through [`clean`] (shared with the GUI) before it reaches the
//! terminal.

pub use organizer::frontend::text::{clean, hint};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// Display width in terminal columns (CJK characters count as two).
pub fn width(text: &str) -> usize {
    UnicodeWidthStr::width(text)
}

/// Truncates at the end with `…` to at most `max` columns.
pub fn tail(text: &str, max: usize) -> String {
    if width(text) <= max {
        return text.to_owned();
    }
    let mut out = String::new();
    let mut used = 0;
    for c in text.chars() {
        let w = c.width().unwrap_or(0);
        if used + w + 1 > max {
            break;
        }
        out.push(c);
        used += w;
    }
    out.push('…');
    out
}

/// Elides the middle of a path, keeping the start and the file name.
pub fn middle(text: &str, max: usize) -> String {
    if width(text) <= max || max < 5 {
        return tail(text, max);
    }
    let keep = max - 1;
    let right_budget = keep / 2 + keep % 2;
    let mut right: Vec<char> = vec![];
    let mut used = 0;
    for c in text.chars().rev() {
        let w = c.width().unwrap_or(0);
        if used + w > right_budget {
            break;
        }
        right.push(c);
        used += w;
    }
    right.reverse();
    let left = tail(text, keep - used + 1);
    let left = left.trim_end_matches('…');
    format!("{left}…{}", right.into_iter().collect::<String>())
}

/// Pads with spaces to exactly `columns` (truncating if needed).
pub fn pad(text: &str, columns: usize) -> String {
    let text = tail(text, columns);
    let fill = columns.saturating_sub(width(&text));
    format!("{text}{}", " ".repeat(fill))
}

/// Escaped last path component, for messages.
pub fn file_name(path: &std::path::Path) -> String {
    clean(
        &path
            .file_name()
            .map_or_else(|| path.to_string_lossy(), |n| n.to_string_lossy()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn widths_and_truncation_respect_cjk() {
        assert_eq!(width("论文ab"), 6);
        assert_eq!(tail("论文论文", 5), "论文…");
        assert_eq!(pad("论", 4), "论  ");
        let path = middle("Research/Papers/Security/ASLR.pdf", 20);
        assert!(
            width(&path) <= 20 && path.ends_with("ASLR.pdf") && path.contains('…'),
            "{path}"
        );
    }
}
