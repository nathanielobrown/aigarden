//! The rule contract every lint implements, and the registry that drives both
//! the engine and `ailint rules`.
//!
//! A rule is a deep module: given a read-only snapshot of the repository
//! ([`RuleContext`]), it returns every finding it can see. Rules decide their own
//! internal parallelism. Adding a rule here is the only wiring step — the engine
//! and the `rules` listing pick it up automatically.

use std::path::Path;

use crate::config::Config;
use crate::diagnostic::Diagnostic;
use crate::walk::SourceFile;

mod file_length;
mod reference_rules;
mod resolve;

/// Read-only view of the repository handed to every rule.
pub(crate) struct RuleContext<'a> {
    /// Every walked, non-excluded file with its content already read.
    pub(crate) files: &'a [SourceFile],
    pub(crate) config: &'a Config,
    /// The scan root, for resolving repo-root-relative references.
    pub(crate) root: &'a Path,
}

/// A lint rule: named, self-describing, individually toggleable, all-reporting.
pub(crate) trait Rule: Sync {
    /// Stable kebab-case identifier used in config keys and diagnostics.
    fn name(&self) -> &'static str;
    /// One-line description shown by `ailint rules`.
    fn description(&self) -> &'static str;
    /// Whether this rule runs, given config. Defaults to always-on.
    fn enabled(&self, _config: &Config) -> bool {
        true
    }
    /// Every finding this rule sees in `ctx`.
    fn check(&self, ctx: &RuleContext<'_>) -> Vec<Diagnostic>;
}

/// All registered rules. The single list the engine iterates and `rules` prints.
pub(crate) fn registry() -> Vec<Box<dyn Rule>> {
    vec![
        Box::new(file_length::FileLength),
        Box::new(reference_rules::LinkTarget),
        Box::new(reference_rules::LinkCase),
        Box::new(reference_rules::BarePath),
        Box::new(reference_rules::ImportTarget),
        Box::new(reference_rules::CodeDocRef),
    ]
}
