//! The rule contract every lint implements, and the registry that drives both
//! the engine and `aigarden rules`.
//!
//! A rule is a deep module: given a read-only snapshot of the repository
//! ([`RuleContext`]), it returns every finding it can see. Rules decide their own
//! internal parallelism. Adding a rule here is the only wiring step — the engine
//! and the `rules` listing pick it up automatically.

use std::collections::HashSet;
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
pub(crate) mod status_header;

/// Read-only view of the repository handed to every rule. Carries the whole walked
/// file set plus a [`Resolver`]; a rule reads global options through the resolver and
/// self-gates each file with [`Resolver::is_enabled`], so `ignore` and
/// `[per-file-ignores]` (disabling a rule for a path) are honored uniformly.
pub(crate) struct RuleContext<'a> {
    /// Every walked, globally-non-excluded file with its content already read.
    pub(crate) files: &'a [SourceFile],
    pub(crate) config: &'a Config,
    /// Per-file config resolver applying override precedence.
    pub(crate) resolver: &'a Resolver<'a>,
    /// The scan root, for resolving repo-root-relative references.
    pub(crate) root: &'a Path,
    /// Repo-relative paths of terminal-status "frozen" tracker docs (empty when the
    /// `[status-header]` mechanism is inert). Computed once by the engine.
    pub(crate) frozen: &'a HashSet<String>,
}

impl<'a> RuleContext<'a> {
    pub(crate) fn new(
        files: &'a [SourceFile],
        config: &'a Config,
        resolver: &'a Resolver<'a>,
        root: &'a Path,
        frozen: &'a HashSet<String>,
    ) -> Self {
        Self {
            files,
            config,
            resolver,
            root,
            frozen,
        }
    }

    /// True when `rule`'s finding on `rel_path` is suppressed by the frozen
    /// exemption: the file is a terminal-status doc and `rule` is in
    /// `[status-header] suppresses`. The single seam the frozen-aware rules consult.
    pub(crate) fn frozen_suppressed(&self, rule: &str, rel_path: &str) -> bool {
        self.frozen.contains(rel_path)
            && self
                .config
                .status_header
                .suppresses
                .iter()
                .any(|r| r == rule)
    }
}

/// A lint rule: named, self-describing, all-reporting. Enablement is per file — a
/// rule iterates [`RuleContext::files`] and skips any file its resolver reports
/// disabled, so `ignore` / `[per-file-ignores]` can turn a rule off for a glob.
pub(crate) trait Rule: Sync {
    /// Stable kebab-case identifier used in config keys and diagnostics.
    fn name(&self) -> &'static str;
    /// One-line description shown by `aigarden rules`.
    fn description(&self) -> &'static str;
    /// The full contract `aigarden explain <name>` prints. The rule owns its own
    /// documentation, so `explain` and the `rules` status column have one source.
    fn explain(&self) -> Explanation;
    /// Every finding this rule sees in `ctx`.
    fn check(&self, ctx: &RuleContext<'_>) -> Vec<Diagnostic>;
}

/// The full, printable contract for a rule — what `aigarden explain <name>` shows and
/// where the `rules` status column comes from. Held on the rule so a rule is the
/// single definition of its own behavior.
pub(crate) struct Explanation {
    /// What the rule checks and why it matters, in prose.
    pub(crate) checks: &'static str,
    /// Config keys the rule reads under its own `[<name>]` table, each with its
    /// default and purpose. Empty for a rule with no options (toggled only via the
    /// top-level `ignore` / `[per-file-ignores]`).
    pub(crate) config: &'static [ConfigKey],
    /// A representative finding message, so `explain` shows the shape of a hit.
    pub(crate) example: &'static str,
    /// What `aigarden check --fix` does for this rule, or `None` when it has no
    /// autofix (a finding needs a human decision).
    pub(crate) fix: Option<&'static str>,
    /// True for a rule on by default but inert until its config is supplied (only
    /// `descriptive-anchor` today) — surfaced as the `config-gated` status.
    pub(crate) config_gated: bool,
}

impl Explanation {
    /// The one-word lifecycle label shown in the `rules` table and `explain` header.
    pub(crate) fn status(&self) -> &'static str {
        if self.fix.is_some() {
            "fixable"
        } else if self.config_gated {
            "config-gated"
        } else {
            "report-only"
        }
    }
}

/// One config key a rule reads, for the `explain` Config section.
pub(crate) struct ConfigKey {
    pub(crate) key: &'static str,
    pub(crate) default: &'static str,
    pub(crate) purpose: &'static str,
}

/// The config surface of a rule with no options of its own — the reference rules.
/// Toggle these with the top-level `ignore` / `[per-file-ignores]`.
pub(crate) const NO_CONFIG: &[ConfigKey] = &[];

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
        Box::new(status_header::StatusHeader),
    ]
}

/// Every registered rule's name — the single source for validating config rule
/// references (`ignore`, `[per-file-ignores]`) against real rules.
pub(crate) fn rule_names() -> Vec<&'static str> {
    registry().iter().map(|r| r.name()).collect()
}
