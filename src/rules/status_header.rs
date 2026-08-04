//! `status-header`: the terminal-status "frozen docs" contract and the frozen set
//! the reference rules consult.
//!
//! A tracker doc (an issue or plan) carries its lifecycle state in a
//! `**Status:** <value>` header — location is not canonical, the header is. This
//! module owns the one parser of that header, the status vocabulary matching, and
//! the two things the rest of the tool needs from it:
//!
//! - **the frozen set** ([`frozen_files`]) — the terminal-status docs, whose path
//!   citations the rules in `[status-header] suppresses` skip. A closed issue or a
//!   shipped plan is an as-built snapshot pinned to the paths of its era; flagging
//!   its historical references as broken is noise.
//! - **the validation findings** (the [`StatusHeader`] rule) — every scanned doc
//!   must carry a status in the configured vocabulary; a missing or unrecognized
//!   status is reported, never silently treated as non-frozen.
//!
//! The exemption is keyed off *status*, not a path, so it survives a closed item
//! living wherever its tracker keeps it. Which rules it may cover is the rules'
//! own declaration ([`crate::rules::Rule::frozen_aware`]): the markdown citation
//! rules, not the structural ones. A repo that wants a frozen doc's links checked
//! simply leaves `link-target` out of `suppresses`.

use std::collections::HashSet;

use serde::Deserialize;

use crate::diagnostic::Diagnostic;
use crate::references::is_markdown;
use crate::rules::{ConfigKey, Explanation, Rule, RuleContext};
use crate::walk::{SourceFile, build_glob_set};

/// `status-header`: the "frozen docs" contract. A tracker doc (an issue/plan)
/// carries its lifecycle state in a `**Status:** <value>` header, not its folder.
/// A **terminal** status (e.g. `done`, `implemented`) marks the doc *frozen*: it is
/// kept as history and may legitimately cite now-gone paths, so its reference
/// citations are exempt from the rules named in [`Self::suppresses`]. Every scanned
/// doc must carry a status in `live ∪ terminal` or the `status-header` rule reports
/// it (fail loud, never a silent skip).
///
/// **Inert until configured**: with no `files` the whole mechanism does nothing, so
/// it is safe on by default. A repo-wide contract, though `[per-file-ignores]` can
/// exempt a specific doc from the header requirement.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct StatusHeaderConfig {
    /// Globs (repo-relative) of the docs under the contract, e.g.
    /// `["issues/**/*.md", "plans/*.md"]`. A `README.md` under a matched glob is
    /// never treated as a tracked item. Empty ⇒ the mechanism is inert.
    #[serde(default)]
    pub files: Vec<String>,
    /// The bold header label; the parser matches `**<header>:** <value>` at line
    /// start and reads the leading keyword of `<value>` (trailing bold fields like
    /// `**Opened:** …` and a date suffix are ignored).
    #[serde(default = "default_status_header")]
    pub header: String,
    /// Live (non-frozen) status keywords, matched leading-keyword and
    /// case-insensitively. Longest keyword wins, so `open question` beats `open`.
    #[serde(default)]
    pub live: Vec<String>,
    /// Terminal (frozen) status keywords. A doc whose status matches one of these
    /// is frozen; its citations are exempt from [`Self::suppresses`].
    #[serde(default)]
    pub terminal: Vec<String>,
    /// Which rules the frozen exemption suppresses on a terminal-status doc.
    /// Validated at load against the rules that declare themselves frozen-aware
    /// ([`crate::rules::frozen_aware_rules`]) — a typo or a structurally-inapplicable
    /// rule is a loud config error, never a silent no-op.
    #[serde(default)]
    pub suppresses: Vec<String>,
}

impl Default for StatusHeaderConfig {
    fn default() -> Self {
        Self {
            files: Vec::new(),
            header: default_status_header(),
            live: Vec::new(),
            terminal: Vec::new(),
            suppresses: Vec::new(),
        }
    }
}

impl StatusHeaderConfig {
    /// The full recognized vocabulary (live then terminal), for messages and matching.
    #[must_use]
    pub fn vocabulary(&self) -> Vec<&str> {
        self.live
            .iter()
            .chain(&self.terminal)
            .map(String::as_str)
            .collect()
    }
    /// True when the mechanism actually runs (given files to scan). Inert with no
    /// `files`; a repo turns the rule off entirely via top-level `ignore` instead.
    #[must_use]
    pub fn active(&self) -> bool {
        !self.files.is_empty()
    }
}

fn default_status_header() -> String {
    "Status".to_string()
}

/// How one scanned doc's status classified against the configured vocabulary.
#[derive(Debug, PartialEq, Eq)]
enum StatusClass {
    /// A live (non-frozen) status.
    Live,
    /// A terminal (frozen) status — the doc joins the frozen set.
    Terminal,
    /// No `**<header>:**` line at all.
    Missing,
    /// A `**<header>:**` line whose value matches no vocabulary keyword.
    Unknown(String),
}

/// The value of the first `**<header>:** <value>` line, trimmed, or `None`. Matches
/// the source tool's `^\*\*Status:\*\*\s+(.+?)\s*$`: anchored at line start, one-plus
/// whitespace after the label, a non-empty value (trailing bold fields included —
/// keyword matching reads only the leading word).
fn status_value(content: &str, header: &str) -> Option<String> {
    let prefix = format!("**{header}:**");
    for line in content.lines() {
        if let Some(rest) = line.strip_prefix(&prefix)
            && rest.starts_with(char::is_whitespace)
        {
            let value = rest.trim();
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

/// The longest vocabulary keyword `value` begins with (case-insensitive), bounded so
/// `open` does not match inside `opened` — the next char after the keyword must not
/// be a lowercase letter or `-`. Longest match wins so `open question` beats `open`.
fn match_keyword<'a>(value: &str, vocab: &[&'a str]) -> Option<&'a str> {
    let lowered = value.to_ascii_lowercase();
    vocab
        .iter()
        .copied()
        .filter(|kw| {
            let kwl = kw.to_ascii_lowercase();
            lowered.starts_with(&kwl)
                && match lowered[kwl.len()..].chars().next() {
                    Some(c) => !(c.is_ascii_lowercase() || c == '-'),
                    None => true,
                }
        })
        .max_by_key(|kw| kw.len())
}

/// Classify a doc's status against the vocabulary. Terminal and live are matched
/// independently and the longer keyword wins (the source tool's whole-vocabulary
/// longest-match, then classify-by-set), so a value is frozen only when its best
/// match is a terminal keyword.
fn classify(content: &str, cfg: &StatusHeaderConfig) -> StatusClass {
    let Some(value) = status_value(content, &cfg.header) else {
        return StatusClass::Missing;
    };
    let terminal: Vec<&str> = cfg.terminal.iter().map(String::as_str).collect();
    let live: Vec<&str> = cfg.live.iter().map(String::as_str).collect();
    match (
        match_keyword(&value, &terminal),
        match_keyword(&value, &live),
    ) {
        (Some(t), Some(l)) => {
            if t.len() >= l.len() {
                StatusClass::Terminal
            } else {
                StatusClass::Live
            }
        }
        (Some(_), None) => StatusClass::Terminal,
        (None, Some(_)) => StatusClass::Live,
        (None, None) => StatusClass::Unknown(value),
    }
}

/// A `README.md` is prose about the tracker, not a tracked item — never scanned
/// (matches the source tool). Basename check, any directory.
fn is_readme(rel_path: &str) -> bool {
    rel_path.rsplit('/').next() == Some("README.md")
}

/// Iterate the scanned tracker docs paired with their status classification: the
/// walked markdown files matching a `files` glob, minus READMEs. The one place the
/// frozen set and the validation findings agree on *which* docs are under the
/// contract. Empty when the mechanism is inert.
fn scanned<'a>(
    files: &'a [SourceFile],
    cfg: &'a StatusHeaderConfig,
) -> Vec<(&'a SourceFile, StatusClass)> {
    if !cfg.active() {
        return Vec::new();
    }
    // Globs are validated at config load, so a build error here is unreachable.
    let matcher = build_glob_set(&cfg.files).expect("status-header files globs validated at load");
    files
        .iter()
        .filter(|f| {
            is_markdown(&f.rel_path) && !is_readme(&f.rel_path) && matcher.is_match(&f.rel_path)
        })
        .map(|f| (f, classify(&f.content, cfg)))
        .collect()
}

/// The frozen set: repo-relative paths of terminal-status tracker docs. Empty when
/// `[status-header]` is inert. The reference rules skip a file in this set for any
/// rule listed in `[status-header] suppresses`.
pub(crate) fn frozen_files(files: &[SourceFile], cfg: &StatusHeaderConfig) -> HashSet<String> {
    scanned(files, cfg)
        .into_iter()
        .filter(|(_, class)| *class == StatusClass::Terminal)
        .map(|(f, _)| f.rel_path.clone())
        .collect()
}

/// `status-header`: every tracker doc under the contract carries a valid status.
pub(crate) struct StatusHeader;

impl Rule for StatusHeader {
    fn name(&self) -> &'static str {
        "status-header"
    }
    fn description(&self) -> &'static str {
        "every issue/plan doc carries a recognized `**Status:**` header"
    }
    fn explain(&self) -> Explanation {
        Explanation {
            checks: "Every doc matching `[status-header] files` carries a `**Status:** <value>` \
header whose leading keyword is in `live ∪ terminal`. A missing or unrecognized status is \
reported, never silently treated as non-frozen. A terminal status marks the doc *frozen*: its \
path citations are exempt from the rules in `suppresses`. Config-driven and inert until `files` \
is set; a repo-wide contract, though `[per-file-ignores]` can exempt a specific doc.",
            config: &[
                ConfigKey {
                    key: "files",
                    default: "none (rule inert)",
                    purpose: "globs of docs under the contract, e.g. [\"issues/**/*.md\", \
\"plans/*.md\"]; a README under a matched glob is never a tracked item",
                },
                ConfigKey {
                    key: "header",
                    default: "\"Status\"",
                    purpose: "the bold label parsed as `**<header>:** <value>`",
                },
                ConfigKey {
                    key: "live",
                    default: "none",
                    purpose: "live (non-frozen) status keywords, e.g. [\"open\", \"active\"]",
                },
                ConfigKey {
                    key: "terminal",
                    default: "none",
                    purpose: "terminal (frozen) status keywords, e.g. [\"done\", \"implemented\"]",
                },
                ConfigKey {
                    key: "suppresses",
                    default: "none",
                    purpose: "rules the frozen exemption skips on a terminal doc; the citation \
rules (link-target, link-case, bare-path, import-target, anchor-resolves, descriptive-anchor) are \
accepted, structural rules are not",
                },
            ],
            example: "status `dnoe` is not a recognized status (expected one of: open, done)",
            fix: None,
            config_gated: true,
        }
    }
    fn check(&self, ctx: &RuleContext<'_>) -> Vec<Diagnostic> {
        let cfg = &ctx.config.status_header;
        let vocab = cfg.vocabulary().join(", ");
        let mut diagnostics = Vec::new();
        for (file, class) in scanned(ctx.files, cfg) {
            // A doc exempted via `[per-file-ignores]` gets no header requirement.
            if !ctx.resolver.is_enabled(self.name(), &file.rel_path) {
                continue;
            }
            let message = match class {
                StatusClass::Live | StatusClass::Terminal => continue,
                StatusClass::Missing => {
                    format!(
                        "missing a `**{}:**` header (expected one of: {vocab})",
                        cfg.header
                    )
                }
                StatusClass::Unknown(value) => {
                    format!(
                        "status `{value}` is not a recognized status (expected one of: {vocab})"
                    )
                }
            };
            diagnostics.push(Diagnostic {
                rule: self.name(),
                path: file.rel_path.clone(),
                span: None,
                message,
                suggestion: Some(
                    "set a `**Status:**` header to a keyword in `[status-header] live`/`terminal`"
                        .to_string(),
                ),
            });
        }
        diagnostics
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A mycelia-shaped config: real issue/plan globs and vocabulary.
    fn cfg() -> StatusHeaderConfig {
        StatusHeaderConfig {
            files: vec!["issues/**/*.md".to_string(), "plans/*.md".to_string()],
            header: "Status".to_string(),
            live: [
                "open",
                "needs-design",
                "in-progress",
                "active",
                "open question",
            ]
            .map(str::to_string)
            .to_vec(),
            terminal: ["done", "wontfix", "implemented", "superseded"]
                .map(str::to_string)
                .to_vec(),
            suppresses: ["bare-path", "link-case", "descriptive-anchor"]
                .map(str::to_string)
                .to_vec(),
        }
    }

    #[test]
    fn terminal_status_with_trailing_bold_fields_is_frozen() {
        // A real closed issue: `**Status:** done **Opened:** 2026-07-10`. Only the
        // leading keyword is read, so the trailing bold field is ignored.
        let content = "# 0065: Something\n\n**Status:** done **Opened:** 2026-07-10\n";
        assert_eq!(classify(content, &cfg()), StatusClass::Terminal);
    }

    #[test]
    fn implemented_with_a_date_suffix_is_frozen() {
        // Plans carry a date: `implemented (2026-06-10)` — the keyword still matches.
        let content = "# Plan\n\n**Status:** implemented (2026-06-10)\n";
        assert_eq!(classify(content, &cfg()), StatusClass::Terminal);
    }

    #[test]
    fn active_status_is_live_not_frozen() {
        let content = "# Plan\n\n**Status:** active — settled design\n";
        assert_eq!(classify(content, &cfg()), StatusClass::Live);
    }

    #[test]
    fn two_word_status_beats_its_single_word_prefix() {
        // `open question` (live) must win over the shorter `open`, so it classifies
        // as live rather than accidentally matching a different keyword.
        let content = "# Plan\n\n**Status:** open question\n";
        assert_eq!(classify(content, &cfg()), StatusClass::Live);
    }

    #[test]
    fn keyword_boundary_rejects_a_longer_word() {
        // `opened` must not match the `open` keyword — the boundary check guards it.
        assert_eq!(match_keyword("opened today", &["open"]), None);
    }

    #[test]
    fn a_missing_header_classifies_missing() {
        let content = "# Issue with no status line\n\nBody.\n";
        assert_eq!(classify(content, &cfg()), StatusClass::Missing);
    }

    #[test]
    fn an_unknown_status_classifies_unknown() {
        let content = "# Issue\n\n**Status:** dnoe\n";
        assert_eq!(
            classify(content, &cfg()),
            StatusClass::Unknown("dnoe".to_string())
        );
    }

    #[test]
    fn frozen_set_collects_terminal_docs_and_skips_readmes() {
        let files = vec![
            source("issues/0001-done.md", "# 0001: X\n\n**Status:** done\n"),
            source("issues/0002-open.md", "# 0002: Y\n\n**Status:** open\n"),
            source("issues/README.md", "# Issues\n\nProse, no status.\n"),
            source(
                "plans/shipped.md",
                "# Plan\n\n**Status:** implemented (2026-01-01)\n",
            ),
            source("docs/guide.md", "# Guide\n\nNot a tracker doc.\n"),
        ];
        let frozen = frozen_files(&files, &cfg());
        assert_eq!(frozen.len(), 2, "two terminal docs: {frozen:?}");
        assert!(frozen.contains("issues/0001-done.md"));
        assert!(frozen.contains("plans/shipped.md"));
        // A live issue, the README, and a non-tracker doc are all absent.
        assert!(!frozen.contains("issues/0002-open.md"));
        assert!(!frozen.contains("issues/README.md"));
        assert!(!frozen.contains("docs/guide.md"));
    }

    #[test]
    fn inert_config_freezes_nothing() {
        // No `files` ⇒ the mechanism does nothing, even with a terminal-looking doc.
        let files = vec![source("issues/0001.md", "# X\n\n**Status:** done\n")];
        assert!(frozen_files(&files, &StatusHeaderConfig::default()).is_empty());
    }

    fn source(rel: &str, content: &str) -> SourceFile {
        SourceFile {
            rel_path: rel.to_string(),
            abs_path: std::path::PathBuf::from(rel),
            content: content.to_string(),
        }
    }
}
