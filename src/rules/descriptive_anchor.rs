//! `descriptive-anchor`: a stable-ID link whose visible text is *only* the bare ID
//! (`ADR-0026`, `T4`, `P2`) reads badly when the link is a sentence's own subject —
//! "as [ADR-0026] argues" forces the reader to already know what 0026 is. The rule
//! asks for descriptive anchor text ("as [gated publication][ADR-0026] argues"); the
//! ID can stay in the link *target*, which this rule never touches.
//!
//! It is entirely config-driven and generic: the stable-ID shapes live in
//! `[descriptive-anchor] patterns` (regexes), so nothing here is project-specific,
//! and with no patterns the rule is inert. Two shapes are deliberately allowed and
//! never flagged:
//! - a **parenthetical citation** — `(see [ADR-0026])` — where the bare ID reads as
//!   an aside, not the subject; detected by tracking prose parenthesis depth
//! - **descriptive text that merely contains the ID** — the whole-text regex match
//!   means `[ADR-0026 — gated publication]` never matches and is never flagged

use std::collections::HashMap;
use std::ops::Range;

use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use regex::Regex;

use crate::config::DescriptiveAnchorConfig;
use crate::diagnostic::{Diagnostic, Span};
use crate::references::is_markdown;
use crate::rules::{Rule, RuleContext};

pub(crate) struct DescriptiveAnchor;

impl Rule for DescriptiveAnchor {
    fn name(&self) -> &'static str {
        "descriptive-anchor"
    }
    fn description(&self) -> &'static str {
        "a stable-ID link carries descriptive anchor text, not just the bare ID"
    }
    fn check(&self, ctx: &RuleContext<'_>) -> Vec<Diagnostic> {
        // Compile each distinct config table's patterns once, keyed by the table's
        // address (the resolver hands back the very same reference), so overrides
        // sharing a table don't recompile. A bad regex is a loud config error —
        // patterns are author-controlled in ailint.toml.
        let mut compiled: HashMap<usize, Vec<Regex>> = HashMap::new();
        let mut register = |cfg: &DescriptiveAnchorConfig| {
            compiled
                .entry(std::ptr::from_ref(cfg) as usize)
                .or_insert_with(|| compile(cfg));
        };
        register(&ctx.config.descriptive_anchor);
        for ov in &ctx.config.overrides {
            if let Some(cfg) = &ov.descriptive_anchor {
                register(cfg);
            }
        }

        let mut diagnostics = Vec::new();
        for file in ctx.files.iter().filter(|f| is_markdown(&f.rel_path)) {
            let cfg = ctx.resolver.descriptive_anchor(&file.rel_path);
            if !cfg.enabled {
                continue;
            }
            let patterns = &compiled[&(std::ptr::from_ref(cfg) as usize)];
            if patterns.is_empty() {
                continue;
            }
            for (span, text) in bare_id_links(&file.content, patterns) {
                diagnostics.push(Diagnostic {
                    rule: self.name(),
                    path: file.rel_path.clone(),
                    span: Some(Span::from_byte_range(&file.content, span)),
                    message: format!(
                        "link text `{text}` is a bare stable ID — give it descriptive anchor text"
                    ),
                    suggestion: Some(
                        "use a descriptive phrase as the link text; the ID can stay in the target"
                            .to_string(),
                    ),
                });
            }
        }
        diagnostics
    }
}

/// Compile one stable-ID pattern into a whole-text-anchored regex — the exact form
/// the rule matches with, so a pattern like `ADR-\d+` flags `[ADR-0026]` but not
/// `[ADR-0026 explained]`. Shared with config-load validation ([`crate::config`]),
/// so a malformed pattern is a clean exit-2 there rather than a panic here.
pub(crate) fn anchored_pattern(pattern: &str) -> Result<Regex, regex::Error> {
    Regex::new(&format!("^(?:{pattern})$"))
}

/// Compile a config table's patterns into whole-text-anchored regexes. Patterns are
/// validated at config load, so a compile error here is an unreachable invariant.
fn compile(cfg: &DescriptiveAnchorConfig) -> Vec<Regex> {
    cfg.patterns
        .iter()
        .map(|p| anchored_pattern(p).expect("descriptive-anchor patterns validated at config load"))
        .collect()
}

/// Every link whose whole visible text matches a stable-ID pattern and that is not
/// a parenthetical citation, returned as `(link span, matched text)`. Parenthesis
/// depth is tracked over prose text (not link text or URLs) and reset at each block
/// start, so an unbalanced paren can't bleed across paragraphs.
fn bare_id_links(content: &str, patterns: &[Regex]) -> Vec<(Range<usize>, String)> {
    let mut out = Vec::new();
    let mut paren_depth: i32 = 0;
    // (link element span, accumulated visible text, whether the link opened inside parens)
    let mut link: Option<(Range<usize>, String, bool)> = None;
    for (event, range) in Parser::new_ext(content, Options::all()).into_offset_iter() {
        match event {
            Event::Start(Tag::Paragraph | Tag::Heading { .. } | Tag::Item) => {
                paren_depth = 0;
            }
            Event::Start(Tag::Link { .. }) => {
                link = Some((range.clone(), String::new(), paren_depth > 0));
            }
            Event::Text(text) => {
                if let Some((_, acc, _)) = link.as_mut() {
                    acc.push_str(&text);
                } else {
                    for c in text.chars() {
                        match c {
                            '(' => paren_depth += 1,
                            ')' => paren_depth = (paren_depth - 1).max(0),
                            _ => {}
                        }
                    }
                }
            }
            // A backticked ID (`` [`ADR-0026`] ``) is still a bare ID as anchor text.
            Event::Code(code) => {
                if let Some((_, acc, _)) = link.as_mut() {
                    acc.push_str(&code);
                }
            }
            Event::End(TagEnd::Link) => {
                if let Some((span, text, in_paren)) = link.take() {
                    let trimmed = text.trim();
                    if !in_paren && patterns.iter().any(|re| re.is_match(trimmed)) {
                        out.push((span, trimmed.to_string()));
                    }
                }
            }
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The mycelia-style stable-ID shapes, as a caller would configure them.
    fn patterns() -> Vec<Regex> {
        ["ADR-\\d+", "T\\d+", "P\\d+"]
            .iter()
            .map(|p| Regex::new(&format!("^(?:{p})$")).unwrap())
            .collect()
    }

    #[test]
    fn flags_a_bare_id_used_as_a_subject() {
        // The link text is *only* the ID and the link is the clause's subject —
        // exactly the unreadable case the rule exists to catch.
        let hits = bare_id_links(
            "As [ADR-0026](docs/adrs/0026.md) argues, ...\n",
            &patterns(),
        );
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].1, "ADR-0026");
    }

    #[test]
    fn allows_a_parenthetical_citation() {
        // Inside parens the bare ID reads as an aside, not the subject — allowed.
        let hits = bare_id_links(
            "Branches are scratch space (see [ADR-0026](docs/adrs/0026.md)).\n",
            &patterns(),
        );
        assert!(hits.is_empty(), "citation in parens is fine: {hits:?}");
    }

    #[test]
    fn allows_descriptive_text_containing_the_id() {
        // Whole-text match: extra words mean the text is descriptive, not a bare ID.
        let hits = bare_id_links(
            "See [ADR-0026 gated publication](docs/adrs/0026.md).\n",
            &patterns(),
        );
        assert!(hits.is_empty(), "descriptive anchor is fine: {hits:?}");
    }

    #[test]
    fn flags_a_backticked_bare_id() {
        // A code-span ID as the whole link text is still a bare ID.
        let hits = bare_id_links(
            "Per [`T4`](docs/theses.md#t4) the set is small.\n",
            &patterns(),
        );
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].1, "T4");
    }

    #[test]
    fn a_paren_in_one_block_does_not_leak_into_the_next() {
        // An unbalanced `(` in one paragraph must not suppress a bare ID in a later
        // paragraph — paren depth resets at each block.
        let content = "An open paren ( here.\n\n[P2](docs/roadmap.md) is next.\n";
        let hits = bare_id_links(content, &patterns());
        assert_eq!(hits.len(), 1, "second block still checked: {hits:?}");
        assert_eq!(hits[0].1, "P2");
    }

    #[test]
    fn no_patterns_means_no_findings() {
        let hits = bare_id_links("As [ADR-0026](docs/adrs/0026.md) argues.\n", &[]);
        assert!(hits.is_empty());
    }
}
