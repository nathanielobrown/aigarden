//! The rule contract every lint implements, and the registry that drives both
//! the engine and `ailint rules`.
//!
//! A rule is a deep module: given a read-only snapshot of the repository
//! ([`RuleContext`]), it returns every finding it can see. Rules decide their own
//! internal parallelism. Adding a rule here is the only wiring step — the engine
//! and the `rules` listing pick it up automatically.

use std::path::Path;

use crate::config::{Config, Resolver};
use crate::diagnostic::Diagnostic;
use crate::walk::SourceFile;

mod cog_fresh;
pub(crate) mod descriptive_anchor;
mod file_length;
mod reference_rules;
pub(crate) mod resolve;
mod rumdl_rules;

/// Read-only view of the repository handed to every rule. Carries the whole walked
/// file set plus a [`Resolver`]; a rule reads a file's effective config through the
/// resolver and self-gates per file, so glob-scoped [`crate::config::Override`]s
/// (including disabling a rule for a path) are honored uniformly.
pub(crate) struct RuleContext<'a> {
    /// Every walked, globally-non-excluded file with its content already read.
    pub(crate) files: &'a [SourceFile],
    pub(crate) config: &'a Config,
    /// Per-file config resolver applying override precedence.
    pub(crate) resolver: &'a Resolver<'a>,
    /// The scan root, for resolving repo-root-relative references.
    pub(crate) root: &'a Path,
}

impl<'a> RuleContext<'a> {
    pub(crate) fn new(
        files: &'a [SourceFile],
        config: &'a Config,
        resolver: &'a Resolver<'a>,
        root: &'a Path,
    ) -> Self {
        Self {
            files,
            config,
            resolver,
            root,
        }
    }
}

/// A lint rule: named, self-describing, all-reporting. Enablement is per file, not
/// per rule — a rule iterates [`RuleContext::files`] and skips any file its
/// resolver reports disabled, so overrides can turn a rule off for one glob.
pub(crate) trait Rule: Sync {
    /// Stable kebab-case identifier used in config keys and diagnostics.
    fn name(&self) -> &'static str;
    /// One-line description shown by `ailint rules`.
    fn description(&self) -> &'static str;
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
        Box::new(rumdl_rules::AnchorResolves),
        Box::new(rumdl_rules::MarkdownStyle),
        Box::new(cog_fresh::CogFresh),
        Box::new(descriptive_anchor::DescriptiveAnchor),
    ]
}
