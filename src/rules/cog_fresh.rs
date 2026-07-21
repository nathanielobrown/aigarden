//! `cog-fresh`: every generated cog block matches what its generator would
//! produce now. This is the cog engine's `--check` surfaced through the registry,
//! so `ailint check` gates cog freshness alongside every other rule.
//!
//! A failing generator becomes a finding here (never a hard error), so one broken
//! cog cannot abort the whole `check` run — the standalone `ailint cog --check`
//! is the path that treats a generator failure as a tool error.

use crate::cog;
use crate::config::Config;
use crate::diagnostic::Diagnostic;
use crate::references::is_markdown;
use crate::rules::{Rule, RuleContext};

pub(crate) struct CogFresh;

impl Rule for CogFresh {
    fn name(&self) -> &'static str {
        "cog-fresh"
    }
    fn description(&self) -> &'static str {
        "a generated cog block matches what its generator produces now"
    }
    fn enabled(&self, config: &Config) -> bool {
        config.cog_fresh.enabled
    }
    fn check(&self, ctx: &RuleContext<'_>) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();
        for file in ctx.files.iter().filter(|f| is_markdown(&f.rel_path)) {
            let root = cog::repo_root(file.abs_path.parent().unwrap_or(&file.abs_path), ctx.root);
            for finding in cog::evaluate(&file.content, &file.abs_path, &root) {
                diagnostics.push(cog::to_diagnostic(&file.rel_path, &file.content, &finding));
            }
        }
        diagnostics
    }
}
