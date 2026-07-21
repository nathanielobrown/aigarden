//! The two rules backed by the `rumdl` library (see [`crate::rumdl_adapter`]):
//! `anchor-resolves` (MD051 link-fragment resolution) and `markdown-style` (a
//! curated, auto-fixable slice of rumdl's style checks). Both are thin: they pick
//! a rumdl rule set, run it through the adapter, and map warnings to ailint
//! diagnostics. Nothing else in the tree touches rumdl types.

use crate::diagnostic::{Diagnostic, Span};
use crate::rules::{ConfigKey, ENABLED_KEY, ENABLED_ONLY, Explanation, Rule, RuleContext};
use crate::rumdl_adapter::{RumdlFinding, anchor_rules, char_pos_to_byte, run, style_rules};

/// Map one rumdl warning to an ailint diagnostic, converting its character
/// position to a byte span. `message` lets the caller reshape the raw rumdl text.
fn to_diagnostic(rule: &'static str, finding: &RumdlFinding<'_>, message: String) -> Diagnostic {
    let warning = &finding.warning;
    let content = &finding.file.content;
    let start = char_pos_to_byte(content, warning.line, warning.column);
    let end = char_pos_to_byte(content, warning.end_line, warning.end_column).max(start);
    Diagnostic {
        rule,
        path: finding.file.rel_path.clone(),
        span: Some(Span::from_byte_range(content, start..end)),
        message,
        suggestion: None,
    }
}

/// `anchor-resolves`: a `#fragment` in a markdown link points at a heading that
/// exists — in the same file or, for `other.md#frag`, in the linked file. Backed
/// by rumdl MD051, which owns the heading-slug long tail.
pub(crate) struct AnchorResolves;

impl Rule for AnchorResolves {
    fn name(&self) -> &'static str {
        "anchor-resolves"
    }
    fn description(&self) -> &'static str {
        "a link `#fragment` resolves to a heading in the target file"
    }
    fn explain(&self) -> Explanation {
        Explanation {
            checks: "A markdown link `#fragment` points at a heading that exists — in the same \
file, or for `other.md#frag` in the linked file. Backed by rumdl MD051, which owns the \
heading-slug long tail.",
            config: ENABLED_ONLY,
            example: "Link fragment '#setup' does not have a corresponding heading",
            fix: None,
            config_gated: false,
        }
    }
    fn check(&self, ctx: &RuleContext<'_>) -> Vec<Diagnostic> {
        // Index over *all* markdown files so cross-file (`other.md#frag`) lookups
        // resolve even when the target file has the rule disabled; then suppress
        // findings on files an override turned off.
        run(ctx.files.iter(), &anchor_rules())
            .iter()
            .filter(|finding| ctx.resolver.anchor_resolves(&finding.file.rel_path))
            .map(|finding| to_diagnostic(self.name(), finding, finding.warning.message.clone()))
            .collect()
    }
}

/// `markdown-style`: markdown hygiene surfaced from rumdl — trailing spaces, hard
/// tabs, multiple blank lines, a single trailing newline, and (opt-in) paragraph
/// reflow. All fixable via `ailint check --fix`. The originating rumdl rule id is
/// prefixed onto each message so a finding is traceable.
pub(crate) struct MarkdownStyle;

impl Rule for MarkdownStyle {
    fn name(&self) -> &'static str {
        "markdown-style"
    }
    fn description(&self) -> &'static str {
        "markdown hygiene: trailing spaces, tabs, blank runs, final newline"
    }
    fn explain(&self) -> Explanation {
        Explanation {
            checks: "Markdown hygiene from a curated rumdl slice: trailing spaces, hard tabs, \
multiple blank lines, a single final newline, and opt-in paragraph reflow. Each message is \
prefixed with the originating rumdl rule id.",
            config: &[
                ENABLED_KEY,
                ConfigKey {
                    key: "reflow",
                    default: "false",
                    purpose: "normalize each paragraph to one line (rumdl MD013); off because \
it rewrites prose",
                },
            ],
            example: "[MD009] Trailing whitespace",
            fix: Some(
                "`ailint check --fix` rewrites each file in place: strips trailing spaces, \
converts hard tabs, collapses blank-line runs, and ensures a single final newline (and \
reflows paragraphs when reflow = true).",
            ),
            config_gated: false,
        }
    }
    fn check(&self, ctx: &RuleContext<'_>) -> Vec<Diagnostic> {
        // Style rules are single-file, but the rule *set* depends on each file's
        // resolved `reflow`, so partition the enabled files by reflow and run each
        // group with its own rule set. Files an override disabled are dropped.
        let mut reflow_on: Vec<&crate::walk::SourceFile> = Vec::new();
        let mut reflow_off: Vec<&crate::walk::SourceFile> = Vec::new();
        for file in ctx.files {
            let cfg = ctx.resolver.markdown_style(&file.rel_path);
            if !cfg.enabled {
                continue;
            }
            if cfg.reflow {
                reflow_on.push(file);
            } else {
                reflow_off.push(file);
            }
        }
        let mut diagnostics = Vec::new();
        for (reflow, group) in [(true, reflow_on), (false, reflow_off)] {
            for finding in &run(group.into_iter(), &style_rules(reflow)) {
                let message = match &finding.warning.rule_name {
                    Some(id) => format!("[{id}] {}", finding.warning.message),
                    None => finding.warning.message.clone(),
                };
                diagnostics.push(to_diagnostic(self.name(), finding, message));
            }
        }
        diagnostics
    }
}
