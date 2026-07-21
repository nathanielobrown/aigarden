//! The rule engine: run every enabled rule and accumulate all findings.
//!
//! Never stops at the first failing rule — the whole point is one pass that
//! surfaces every class of problem. Output is sorted for deterministic rendering.

use std::path::Path;

use crate::config::Config;
use crate::diagnostic::Diagnostic;
use crate::rules::{RuleContext, registry};
use crate::walk::SourceFile;

/// Run every enabled rule over `files` and return all findings, sorted by
/// (path, rule, start line) so renderers and snapshots are deterministic.
pub(crate) fn check(files: &[SourceFile], config: &Config, root: &Path) -> Vec<Diagnostic> {
    check_with(files, config, root, |_| true)
}

/// Like [`check`], but only rules whose name satisfies `keep` run. `mv`'s
/// verify-after step uses this to re-run just the reference-integrity rules.
pub(crate) fn check_with(
    files: &[SourceFile],
    config: &Config,
    root: &Path,
    keep: impl Fn(&str) -> bool,
) -> Vec<Diagnostic> {
    let ctx = RuleContext {
        files,
        config,
        root,
    };
    let mut diagnostics: Vec<Diagnostic> = registry()
        .iter()
        .filter(|rule| rule.enabled(config) && keep(rule.name()))
        .flat_map(|rule| rule.check(&ctx))
        .collect();
    diagnostics.sort_by(|a, b| {
        let a_line = a.span.map_or(0, |s| s.start_line);
        let b_line = b.span.map_or(0, |s| s.start_line);
        (a.path.as_str(), a.rule, a_line).cmp(&(b.path.as_str(), b.rule, b_line))
    });
    diagnostics
}
