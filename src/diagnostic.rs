//! The one finding type every rule produces and every renderer consumes.
//!
//! A `Diagnostic` is deliberately renderer-agnostic: it names the rule, the
//! file, an optional source `Span`, a message, and an optional fix hint. The
//! `human`/`json`/`github` renderers each project this same value differently.

use std::ops::Range;

/// A single rule finding, keyed by kebab-case rule name and repo-relative path.
#[derive(Debug, Clone)]
pub struct Diagnostic {
    /// Kebab-case rule name, e.g. `file-length`.
    pub rule: &'static str,
    /// Path relative to the invocation directory, forward-slashed.
    pub path: String,
    /// Where in the file the finding sits; `None` for whole-file findings.
    pub span: Option<Span>,
    /// One-line human-readable description of the finding.
    pub message: String,
    /// Optional actionable hint (rendered as `help:` / json `suggestion`).
    pub suggestion: Option<String>,
}

/// A byte range in a source file plus its 1-based line/column projection.
///
/// Byte offsets drive autofixes and `annotate-snippets`; line/column drive the
/// json and github renderers. Build with [`Span::from_byte_range`] so the two
/// views can never disagree.
#[derive(Debug, Clone, Copy)]
pub struct Span {
    pub start_byte: usize,
    pub end_byte: usize,
    pub start_line: usize,
    pub start_col: usize,
    pub end_line: usize,
    pub end_col: usize,
}

impl Span {
    /// Project a byte range onto 1-based line/column coordinates of `source`.
    #[must_use]
    pub fn from_byte_range(source: &str, range: Range<usize>) -> Self {
        let (start_line, start_col) = line_col(source, range.start);
        let (end_line, end_col) = line_col(source, range.end);
        Self {
            start_byte: range.start,
            end_byte: range.end,
            start_line,
            start_col,
            end_line,
            end_col,
        }
    }
}

/// 1-based (line, column) of `byte` within `source`, counting columns in chars.
fn line_col(source: &str, byte: usize) -> (usize, usize) {
    let mut line = 1;
    let mut col = 1;
    for (idx, ch) in source.char_indices() {
        if idx >= byte {
            break;
        }
        if ch == '\n' {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
    }
    (line, col)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn projects_byte_range_to_line_and_column() {
        // "ab\ncde" — byte 4 is 'd', on line 2 column 2.
        let span = Span::from_byte_range("ab\ncde", 4..5);
        assert_eq!((span.start_line, span.start_col), (2, 2));
        assert_eq!((span.end_line, span.end_col), (2, 3));
    }
}
