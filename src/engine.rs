//! The rule engine: run every enabled rule and accumulate all findings.
//!
//! Never stops at the first failing rule — the whole point is one pass that
//! surfaces every class of problem. Output is sorted for deterministic rendering.

use std::path::Path;

use anyhow::Result;

use crate::config::{Config, Resolver};
use crate::diagnostic::Diagnostic;
use crate::rules::{RuleContext, registry};
use crate::walk::SourceFile;

/// Run every rule over `files` and return all findings, sorted by
/// (path, rule, start line) so renderers and snapshots are deterministic.
pub(crate) fn check(files: &[SourceFile], config: &Config, root: &Path) -> Result<Vec<Diagnostic>> {
    check_with(files, config, root, |_| true)
}

/// Like [`check`], but only rules whose name satisfies `keep` run. `mv`'s
/// verify-after step uses this to re-run just the reference-integrity rules.
///
/// Every rule runs over one shared [`RuleContext`]; enablement is per file, decided
/// by the [`Resolver`] from `ignore` plus `[per-file-ignores]`. Building the resolver
/// compiles each per-file-ignores glob — a malformed glob is a loud tool error (fail
/// fast), never a silent no-op.
pub(crate) fn check_with(
    files: &[SourceFile],
    config: &Config,
    root: &Path,
    keep: impl Fn(&str) -> bool,
) -> Result<Vec<Diagnostic>> {
    let resolver = Resolver::new(config)?;
    // The frozen set (terminal-status docs) is cross-cutting: the status-header rule
    // validates it, and the suppressed reference rules consult it. Compute it once.
    let frozen = crate::rules::status_header::frozen_files(files, &config.status_header);
    let ctx = RuleContext::new(files, config, &resolver, root, &frozen);
    let mut diagnostics: Vec<Diagnostic> = Vec::new();
    for rule in registry().iter().filter(|rule| keep(rule.name())) {
        diagnostics.extend(rule.check(&ctx));
    }
    diagnostics.sort_by(|a, b| {
        let a_line = a.span.map_or(0, |s| s.start_line);
        let b_line = b.span.map_or(0, |s| s.start_line);
        (a.path.as_str(), a.rule, a_line).cmp(&(b.path.as_str(), b.rule, b_line))
    });
    Ok(diagnostics)
}
