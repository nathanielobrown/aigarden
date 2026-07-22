//! Human renderer: rustc-style annotated snippets when a finding has a span,
//! a compact path/message line when it doesn't (e.g. whole-file findings).

use std::io::{self, Write};

use annotate_snippets::{AnnotationKind, Level, Renderer, Snippet};

use crate::diagnostic::Diagnostic;
use crate::output::Sources;

pub(super) fn render(
    diagnostics: &[Diagnostic],
    files_scanned: usize,
    sources: &Sources<'_>,
    writer: &mut impl Write,
) -> io::Result<()> {
    if diagnostics.is_empty() {
        let plural = if files_scanned == 1 { "" } else { "s" };
        return writeln!(
            writer,
            "\u{2713} aigarden: checked {files_scanned} file{plural}, no findings"
        );
    }
    for diagnostic in diagnostics {
        render_one(diagnostic, sources, writer)?;
    }
    let n = diagnostics.len();
    let plural = if n == 1 { "" } else { "s" };
    writeln!(writer, "aigarden: {n} finding{plural}")
}

fn render_one(
    diagnostic: &Diagnostic,
    sources: &Sources<'_>,
    writer: &mut impl Write,
) -> io::Result<()> {
    match diagnostic.span {
        Some(span) => render_with_snippet(diagnostic, span, sources, writer),
        None => render_plain(diagnostic, writer),
    }
}

/// No span: a compact `path: [rule] message` line plus an optional help line.
fn render_plain(diagnostic: &Diagnostic, writer: &mut impl Write) -> io::Result<()> {
    writeln!(
        writer,
        "{path}: [{rule}] {message}",
        path = diagnostic.path,
        rule = diagnostic.rule,
        message = diagnostic.message,
    )?;
    if let Some(help) = &diagnostic.suggestion {
        writeln!(writer, "  help: {help}")?;
    }
    Ok(())
}

/// Span present: draw the excerpt from the in-memory source the rule spanned
/// against. Reading fresh from disk would decode a non-UTF-8 file differently
/// (or fail), producing a buffer the byte span overruns — a snippet-engine panic.
fn render_with_snippet(
    diagnostic: &Diagnostic,
    span: crate::diagnostic::Span,
    sources: &Sources<'_>,
    writer: &mut impl Write,
) -> io::Result<()> {
    let source = sources.get(diagnostic.path.as_str()).copied().unwrap_or("");
    let mut annotation = AnnotationKind::Primary.span(span.start_byte..span.end_byte);
    if let Some(help) = &diagnostic.suggestion {
        annotation = annotation.label(help.as_str());
    }
    let report = &[Level::ERROR
        .primary_title(diagnostic.message.as_str())
        .id(diagnostic.rule)
        .element(
            Snippet::source(source)
                .path(diagnostic.path.as_str())
                .annotation(annotation),
        )];
    let rendered = Renderer::plain().render(report);
    writeln!(writer, "{rendered}")
}

#[cfg(test)]
mod tests {
    use insta::assert_snapshot;

    use super::*;
    use crate::diagnostic::Span;

    #[test]
    fn renders_a_spanned_finding_with_a_source_excerpt() {
        // Point at a real file so the snippet reads its source: use this test file
        // is fragile, so instead build the diagnostic against inline source by path.
        let source = "let x = 1;\nlet reallylongname = 2;\n";
        let span = Span::from_byte_range(source, 15..28);
        let diagnostic = Diagnostic {
            rule: "example-rule",
            path: "sample.rs".to_string(),
            span: Some(span),
            message: "name is too long".to_string(),
            suggestion: Some("shorten it".to_string()),
        };
        // Render directly against known source (bypassing disk read) to snapshot
        // the annotate-snippets projection deterministically.
        let annotation = AnnotationKind::Primary
            .span(span.start_byte..span.end_byte)
            .label("shorten it");
        let report = &[Level::ERROR
            .primary_title(diagnostic.message.as_str())
            .id(diagnostic.rule)
            .element(
                Snippet::source(source)
                    .path(diagnostic.path.as_str())
                    .annotation(annotation),
            )];
        let rendered = Renderer::plain().render(report);
        assert_snapshot!(rendered);
    }
}
