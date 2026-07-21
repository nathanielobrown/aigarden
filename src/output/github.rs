//! GitHub renderer: one workflow-command annotation per finding.
//!
//! Format: `::error file=PATH,line=L,col=C::[rule] message`. Line/col are omitted
//! when the finding has no span. The green path emits nothing (CI stays quiet).

use std::io::{self, Write};

use crate::diagnostic::Diagnostic;

pub(super) fn render(diagnostics: &[Diagnostic], writer: &mut impl Write) -> io::Result<()> {
    for d in diagnostics {
        let message = escape_data(&format!("[{}] {}", d.rule, d.message));
        match d.span {
            Some(span) => writeln!(
                writer,
                "::error file={path},line={line},col={col}::{message}",
                path = d.path,
                line = span.start_line,
                col = span.start_col,
            )?,
            None => writeln!(writer, "::error file={path}::{message}", path = d.path)?,
        }
    }
    Ok(())
}

/// Escape the workflow-command message body (newlines/CR must be percent-encoded).
fn escape_data(s: &str) -> String {
    s.replace('%', "%25")
        .replace('\r', "%0D")
        .replace('\n', "%0A")
}
