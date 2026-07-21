//! JSON renderer: a stable, versioned report schema for agents and tooling.
//!
//! Schema (version 1): `{ version, summary: { files_scanned, findings },
//! diagnostics: [ { rule, path, message, suggestion, span } ] }` where `span` is
//! null for whole-file findings or `{ start_line, start_col, end_line, end_col,
//! start_byte, end_byte }`. Emitted even on the green path (empty `diagnostics`).

use std::io::{self, Write};

use serde::Serialize;

use crate::diagnostic::{Diagnostic, Span};

const SCHEMA_VERSION: u32 = 1;

#[derive(Serialize)]
struct Report<'a> {
    version: u32,
    summary: Summary,
    diagnostics: Vec<DiagnosticView<'a>>,
}

#[derive(Serialize)]
struct Summary {
    files_scanned: usize,
    findings: usize,
}

#[derive(Serialize)]
struct DiagnosticView<'a> {
    rule: &'a str,
    path: &'a str,
    message: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    suggestion: Option<&'a str>,
    span: Option<SpanView>,
}

#[derive(Serialize)]
struct SpanView {
    start_line: usize,
    start_col: usize,
    end_line: usize,
    end_col: usize,
    start_byte: usize,
    end_byte: usize,
}

impl From<Span> for SpanView {
    fn from(s: Span) -> Self {
        Self {
            start_line: s.start_line,
            start_col: s.start_col,
            end_line: s.end_line,
            end_col: s.end_col,
            start_byte: s.start_byte,
            end_byte: s.end_byte,
        }
    }
}

pub(super) fn render(
    diagnostics: &[Diagnostic],
    files_scanned: usize,
    writer: &mut impl Write,
) -> io::Result<()> {
    let report = Report {
        version: SCHEMA_VERSION,
        summary: Summary {
            files_scanned,
            findings: diagnostics.len(),
        },
        diagnostics: diagnostics
            .iter()
            .map(|d| DiagnosticView {
                rule: d.rule,
                path: &d.path,
                message: &d.message,
                suggestion: d.suggestion.as_deref(),
                span: d.span.map(SpanView::from),
            })
            .collect(),
    };
    let json = serde_json::to_string_pretty(&report).expect("report serializes");
    writeln!(writer, "{json}")
}
