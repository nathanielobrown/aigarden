//! The two rules backed by the `rumdl` library (see [`crate::rumdl_adapter`]):
//! `anchor-resolves` (MD051 link-fragment resolution) and `markdown-style` (a
//! curated, auto-fixable slice of rumdl's style checks). Both are thin: they pick
//! a rumdl rule set, run it through the adapter, and map warnings to ailint
//! diagnostics. Nothing else in the tree touches rumdl types.

use crate::config::Config;
use crate::diagnostic::{Diagnostic, Span};
use crate::rules::{Rule, RuleContext};
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
    fn enabled(&self, config: &Config) -> bool {
        config.anchor_resolves.enabled
    }
    fn check(&self, ctx: &RuleContext<'_>) -> Vec<Diagnostic> {
        run(ctx.files, &anchor_rules())
            .iter()
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
        "markdown hygiene: trailing spaces, tabs, blank runs, final newline (fixable)"
    }
    fn enabled(&self, config: &Config) -> bool {
        config.markdown_style.enabled
    }
    fn check(&self, ctx: &RuleContext<'_>) -> Vec<Diagnostic> {
        run(ctx.files, &style_rules(ctx.config.markdown_style.reflow))
            .iter()
            .map(|finding| {
                let message = match &finding.warning.rule_name {
                    Some(id) => format!("[{id}] {}", finding.warning.message),
                    None => finding.warning.message.clone(),
                };
                to_diagnostic(self.name(), finding, message)
            })
            .collect()
    }
}
