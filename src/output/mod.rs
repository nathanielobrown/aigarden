//! Renderers: one `Diagnostic` slice projected three ways.
//!
//! Every renderer writes to an explicit `Write` sink (never `println!`), so the
//! output path is testable and honours the "all output through this layer" rule.
//! Across formats the green path is quiet (one line) and the red path is loud.

use std::io::{self, Write};

use crate::cli::OutputFormat;
use crate::diagnostic::Diagnostic;

mod github;
mod human;
mod json;

/// Render `diagnostics` in `format` to `writer`. `files_scanned` feeds summaries.
pub(crate) fn render(
    format: OutputFormat,
    diagnostics: &[Diagnostic],
    files_scanned: usize,
    writer: &mut impl Write,
) -> io::Result<()> {
    match format {
        OutputFormat::Human => human::render(diagnostics, files_scanned, writer),
        OutputFormat::Json => json::render(diagnostics, files_scanned, writer),
        OutputFormat::Github => github::render(diagnostics, writer),
    }
}
