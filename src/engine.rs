//! The rule engine: run every enabled rule and accumulate all findings.
//!
//! Never stops at the first failing rule — the whole point is one pass that
//! surfaces every class of problem. Output is sorted for deterministic rendering.

use crate::config::Config;
use crate::diagnostic::Diagnostic;
use crate::rules::{RuleContext, registry};
use crate::walk::SourceFile;

/// Run every enabled rule over `files` and return all findings, sorted by
/// (path, rule, start line) so renderers and snapshots are deterministic.
pub(crate) fn check(files: &[SourceFile], config: &Config) -> Vec<Diagnostic> {
    let ctx = RuleContext { files, config };
    let mut diagnostics: Vec<Diagnostic> = registry()
        .iter()
        .filter(|rule| rule.enabled(config))
        .flat_map(|rule| rule.check(&ctx))
        .collect();
    diagnostics.sort_by(|a, b| {
        let a_line = a.span.map_or(0, |s| s.start_line);
        let b_line = b.span.map_or(0, |s| s.start_line);
        (a.path.as_str(), a.rule, a_line).cmp(&(b.path.as_str(), b.rule, b_line))
    });
    diagnostics
}
