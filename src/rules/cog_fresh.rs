//! `cog-fresh`: every generated cog block matches what its generator would
//! produce now. This is the cog engine's `--check` surfaced through the registry,
//! so `aigarden check` gates cog freshness alongside every other rule.
//!
//! A failing generator becomes a finding here (never a hard error), so one broken
//! cog cannot abort the whole `check` run — the standalone `aigarden cog --check`
//! is the path that treats a generator failure as a tool error.

use crate::cog;
use crate::diagnostic::Diagnostic;
use crate::rules::{ConfigKey, Explanation, Rule, RuleContext};

pub(crate) struct CogFresh;

impl Rule for CogFresh {
    fn name(&self) -> &'static str {
        "cog-fresh"
    }
    fn description(&self) -> &'static str {
        "a generated cog block matches what its generator produces now"
    }
    fn explain(&self) -> Explanation {
        Explanation {
            checks: "A generated cog block (`<!-- aigarden:cog … -->` … `<!-- aigarden:end -->`) \
matches what its generator produces now. Regenerate stale blocks with `aigarden cog --write`. A \
failing generator becomes a finding here rather than aborting the whole check run. Covers \
markdown files plus any `extend-include` match; `aigarden cog` acts on the same files.",
            config: &[ConfigKey {
                key: "extend-include",
                default: "[]",
                purpose: "globs of non-markdown files (e.g. `.codex/agents/*.toml`) whose cog \
blocks are also checked and written; `aigarden cog` exits 2 if one matches no file",
            }],
            example: "cog block is stale — its generator now produces different output",
            fix: None,
            config_gated: false,
        }
    }
    fn check(&self, ctx: &RuleContext<'_>) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();
        for file in ctx
            .files
            .iter()
            .filter(|f| cog::is_cog_file(&f.rel_path, ctx.resolver))
        {
            let root = cog::repo_root(file.abs_path.parent().unwrap_or(&file.abs_path), ctx.root);
            for finding in cog::evaluate(&file.content, &file.abs_path, &root) {
                diagnostics.push(cog::to_diagnostic(&file.rel_path, &file.content, &finding));
            }
        }
        diagnostics
    }
}
