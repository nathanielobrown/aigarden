//! Renderers: one `Diagnostic` slice projected three ways.
//!
//! Every renderer writes to an explicit `Write` sink (never `println!`), so the
//! output path is testable and honours the "all output through this layer" rule.
//! Across formats the green path is quiet (one line) and the red path is loud.

use std::collections::HashMap;
use std::io::{self, Write};

use crate::cli::OutputFormat;
use crate::diagnostic::Diagnostic;

mod github;
mod human;
mod json;

/// Per-path in-memory source text, keyed by a diagnostic's `path`. The human
/// renderer draws its snippet from this — the exact content the rule spanned
/// against — so it never re-reads from disk (which decodes differently for a
/// non-UTF-8 file and would panic the snippet engine on an out-of-range span).
pub(crate) type Sources<'a> = HashMap<&'a str, &'a str>;

/// Build the [`Sources`] lookup from the walked files — `rel_path` → in-memory
/// `content`. Every `render` caller passes this so the human renderer's snippets
/// read the exact bytes the rules spanned against.
pub(crate) fn sources_from(files: &[crate::walk::SourceFile]) -> Sources<'_> {
    files
        .iter()
        .map(|f| (f.rel_path.as_str(), f.content.as_str()))
        .collect()
}

/// Render `diagnostics` in `format` to `writer`. `sources` supplies the in-memory
/// content the human renderer's snippets read; `files_scanned` feeds summaries.
pub(crate) fn render(
    format: OutputFormat,
    diagnostics: &[Diagnostic],
    files_scanned: usize,
    sources: &Sources<'_>,
    writer: &mut impl Write,
) -> io::Result<()> {
    match format {
        OutputFormat::Human => human::render(diagnostics, files_scanned, sources, writer),
        OutputFormat::Json => json::render(diagnostics, files_scanned, writer),
        OutputFormat::Github => github::render(diagnostics, writer),
    }
}
