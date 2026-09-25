//! Adapter over the `rumdl` markdown-lint library — the one place that speaks
//! rumdl's types. It surfaces two capabilities the shared extractor cannot give
//! us cheaply: MD051 anchor resolution (the cross-file fragment check, which
//! needs rumdl's workspace index over the whole file set) and a curated set of
//! auto-fixable style rules.
//!
//! rumdl reports 1-based *character* columns; aigarden spans are byte ranges, so
//! every warning is converted here via [`char_pos_to_byte`]. rumdl lint failures
//! are exceptional (malformed internal state), so we crash loud rather than
//! degrade — a swallowed rumdl error would silently drop real findings.
//!
//! Style warnings are also placed against the file's cog blocks here
//! ([`CogLayout`]), so the check and `--fix` agree on what the generator owns.

use std::ops::Range;
use std::path::Path;

use anyhow::{Context, Result, anyhow};
use rumdl_lib::LintContext;
use rumdl_lib::config::MarkdownFlavor;
use rumdl_lib::rule::{Fix, LintWarning, Rule as RumdlRule};
use rumdl_lib::rules::md013_line_length::md013_config::ReflowMode;
use rumdl_lib::rules::{
    MD009TrailingSpaces, MD010NoHardTabs, MD012NoMultipleBlanks, MD013Config, MD013LineLength,
    MD031BlanksAroundFences, MD047SingleTrailingNewline, MD051LinkFragments,
};
use rumdl_lib::types::LineLength;
use rumdl_lib::utils::fix_utils::{apply_warning_fixes, filter_warnings_by_inline_config};

use crate::cog;
use crate::config::Reflow;
use crate::references::is_markdown;
use crate::walk::SourceFile;

/// One rumdl warning tied back to the aigarden file it came from.
pub(crate) struct RumdlFinding<'a> {
    pub(crate) file: &'a SourceFile,
    pub(crate) warning: LintWarning,
}

/// The MD051 rule (anchor/link-fragment resolution), single- and cross-file.
pub(crate) fn anchor_rules() -> Vec<Box<dyn RumdlRule>> {
    vec![Box::new(MD051LinkFragments::new())]
}

/// The column never-wrap hands MD013. Normalize joins a paragraph and then wraps
/// it at the limit, so "never wrap" is spelled as a limit no real line reaches.
const NEVER_WRAP_COLUMN: usize = 100_000;

/// The curated auto-fixable style set. These are the universally-agreed markdown
/// hygiene rules (trailing spaces, hard tabs, blank-line runs, blank lines around
/// fences, final newline); `reflow` adds paragraph re-wrapping (MD013) in the mode
/// the caller picked.
pub(crate) fn style_rules(reflow: Reflow) -> Vec<Box<dyn RumdlRule>> {
    let mut rules: Vec<Box<dyn RumdlRule>> = vec![
        Box::new(MD009TrailingSpaces::new(2, false)),
        Box::new(MD010NoHardTabs::new(4)),
        Box::new(MD012NoMultipleBlanks::new(1)),
        // `true`: a fence nested in a list item needs its blank lines too — the
        // renderer swallows the fence without them wherever it sits.
        Box::new(MD031BlanksAroundFences::new(true)),
        Box::new(MD047SingleTrailingNewline),
    ];
    if let Some(config) = md013_config(reflow) {
        rules.push(Box::new(MD013LineLength::from_config_struct(config)));
    }
    rules
}

/// The MD013 settings each reflow mode means, or `None` when reflow is off.
fn md013_config(reflow: Reflow) -> Option<MD013Config> {
    match reflow {
        Reflow::Off => None,
        // Re-wrap over-length paragraphs to rumdl's default line-length limit.
        Reflow::Wrap => Some(MD013Config {
            reflow: true,
            ..MD013Config::default()
        }),
        // Normalize joins each paragraph to one line; the huge limit stops it
        // wrapping again. Fences and tables are excluded so their line breaks —
        // content in one, structure in the other — survive a fix untouched.
        Reflow::NeverWrap => Some(MD013Config {
            reflow: true,
            reflow_mode: ReflowMode::Normalize,
            line_length: LineLength::from_const(NEVER_WRAP_COLUMN),
            code_blocks: false,
            tables: false,
            ..MD013Config::default()
        }),
    }
}

/// Run `rules` over the given markdown files, gathering single-file warnings and
/// MD051-style workspace cross-file warnings. The workspace index is keyed by
/// each file's absolute path — the same form MD051 resolves link targets to —
/// so cross-file anchor lookups hit. Callers pass the files they want indexed;
/// non-markdown files are filtered out here.
pub(crate) fn run<'a>(
    files: impl Iterator<Item = &'a SourceFile>,
    rules: &[Box<dyn RumdlRule>],
) -> Vec<RumdlFinding<'a>> {
    let md_files: Vec<&SourceFile> = files.filter(|f| is_markdown(&f.rel_path)).collect();
    let flavor = MarkdownFlavor::Standard;

    // Phase 1: single-file lint + build the workspace index every file contributes to.
    let mut workspace = rumdl_lib::workspace_index::WorkspaceIndex::new();
    let mut indexed = Vec::with_capacity(md_files.len());
    for file in &md_files {
        let (result, index) = rumdl_lib::lint_and_index(
            &file.content,
            rules,
            false,
            flavor,
            Some(file.abs_path.clone()),
            None,
        );
        let warnings =
            result.unwrap_or_else(|e| panic!("rumdl lint failed on {}: {e}", file.rel_path));
        workspace.insert_file(file.abs_path.clone(), index.clone());
        indexed.push((*file, index, warnings));
    }

    // Phase 2: emit single-file warnings, then run cross-file checks against the
    // now-complete workspace (MD051 resolves `other.md#frag` here).
    let mut findings = Vec::new();
    for (file, index, single) in indexed {
        for warning in single {
            findings.push(RumdlFinding { file, warning });
        }
        let cross =
            rumdl_lib::run_cross_file_checks(&file.abs_path, &index, rules, &workspace, None)
                .unwrap_or_else(|e| {
                    panic!("rumdl cross-file check failed on {}: {e}", file.rel_path)
                });
        for warning in cross {
            findings.push(RumdlFinding { file, warning });
        }
    }
    findings
}

/// Apply `rules`' fixes to `content` in order, each on the previous rule's output
/// (the order `rumdl --fix` uses), skipping any fix [`CogLayout`] does not hand
/// to the author. Mirrors each rule's own `fix`, which is check-then-apply for
/// every rule in [`style_rules`].
pub(crate) fn fix(content: &str, path: &Path, rules: &[Box<dyn RumdlRule>]) -> Result<String> {
    let mut content = content.to_string();
    for rule in rules {
        let ctx = LintContext::new(&content, MarkdownFlavor::Standard, Some(path.to_path_buf()));
        if rule.should_skip(&ctx) {
            continue;
        }
        // Re-parse each round: an earlier rule's fix shifts every byte after it.
        let layout =
            CogLayout::parse(&content).context("cannot tell generated text from authored text")?;
        let warnings = rule
            .check(&ctx)
            .map_err(|e| anyhow!("checking with {}: {e}", rule.name()))?;
        let authored: Vec<LintWarning> =
            filter_warnings_by_inline_config(warnings, ctx.inline_config(), rule.name())
                .into_iter()
                .filter(|w| matches!(layout.owner(w), Some(Owner::Author)))
                .collect();
        content = apply_warning_fixes(&content, &authored)
            .map_err(|e| anyhow!("fixing with {}: {e}", rule.name()))?;
    }
    Ok(content)
}

/// Style rules whose findings are "no blank line between this construct and its
/// neighbor". A cog marker counts as that blank line; MD012 (blank-line runs) is
/// deliberately absent, since a marker is a real line there.
const BLANK_NEIGHBOR_RULES: &[&str] = &["MD031"];

/// Who owns the text a style warning sits in, and so who must fix it.
pub(crate) enum Owner {
    /// Authored text: report the warning, and `--fix` may rewrite it.
    Author,
    /// A cog block's generated body, named by its directive: report the warning
    /// against the generator, and never fix it in place.
    Generator(String),
}

/// A file's cog blocks, laid out for placing style warnings. See the "Cogs and
/// markdown style" section of `docs/design.md` for the two rules it enforces.
pub(crate) struct CogLayout {
    /// Byte offset of each line's start; index 0 is line 1.
    line_starts: Vec<usize>,
    blocks: Vec<cog::CogBlock>,
}

impl CogLayout {
    /// Lay out `content`'s cog blocks. A malformed block set is an error: without
    /// the block boundaries, no one can say which text the generator owns.
    pub(crate) fn parse(content: &str) -> Result<Self> {
        let line_starts = std::iter::once(0)
            .chain(content.match_indices('\n').map(|(i, _)| i + 1))
            .filter(|&start| start < content.len())
            .collect();
        Ok(Self {
            line_starts,
            blocks: cog::find_blocks(content)?,
        })
    }

    /// Who owns `warning`'s text, or `None` when a cog marker already supplies
    /// the blank line a blank-neighbor rule asks for.
    pub(crate) fn owner(&self, warning: &LintWarning) -> Option<Owner> {
        let edits = warning
            .fix
            .iter()
            .flat_map(|fix| std::iter::once(fix).chain(&fix.additional_edits));
        if warning
            .rule_name
            .as_deref()
            .is_some_and(|id| BLANK_NEIGHBOR_RULES.contains(&id))
            && edits.clone().any(|edit| self.inserts_beside_marker(edit))
        {
            return None;
        }
        let lines = warning.line..=warning.end_line.max(warning.line);
        let owner = self.blocks.iter().find(|block| {
            let body = &block.body_span;
            let body_lines = self.line_of(body.start)..self.line_of(body.end);
            body_lines.clone().any(|line| lines.contains(&line))
                || edits.clone().any(|edit| touches(&edit.range, body))
        });
        Some(owner.map_or(Owner::Author, |block| Owner::Generator(block.directive())))
    }

    /// Whether `edit` inserts text at a line boundary with a marker on either side.
    fn inserts_beside_marker(&self, edit: &Fix) -> bool {
        let at = edit.range.start;
        if !edit.range.is_empty() || self.line_starts.binary_search(&at).is_err() {
            return false;
        }
        let below = self.line_of(at);
        self.blocks.iter().any(|block| {
            // The end marker line starts where the body ends.
            let markers = [
                self.line_of(block.open_marker_span.start),
                self.line_of(block.body_span.end),
            ];
            markers.contains(&below) || markers.contains(&(below - 1))
        })
    }

    /// The 1-based line number holding byte `offset`.
    fn line_of(&self, offset: usize) -> usize {
        self.line_starts.partition_point(|&start| start <= offset)
    }
}

/// Whether an edit over `range` writes inside `body`. An insertion at either end
/// of the body lands inside it, since the markers stay put around it.
fn touches(range: &Range<usize>, body: &Range<usize>) -> bool {
    if range.is_empty() {
        body.contains(&range.start) || range.start == body.end
    } else {
        range.start < body.end && range.end > body.start
    }
}

/// Convert a rumdl 1-based (`line`, `column`) position — column measured in
/// characters — into a byte offset in `content`. Out-of-range columns clamp to
/// the end of their line's text (excluding the newline).
pub(crate) fn char_pos_to_byte(content: &str, line: usize, column: usize) -> usize {
    let line_start: usize = content
        .split_inclusive('\n')
        .take(line.saturating_sub(1))
        .map(str::len)
        .sum();
    let line_body = content[line_start..].split('\n').next().unwrap_or("");
    let extra = line_body
        .char_indices()
        .nth(column.saturating_sub(1))
        .map_or(line_body.len(), |(byte, _)| byte);
    line_start + extra
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fence_flush_against_cog_markers_still_renders_as_code() {
        // The CommonMark fact behind treating a marker as a blank line: an HTML
        // comment block ends on the line holding `-->`, so a fence right after it
        // opens a code block, and prose right before it is not swallowed.
        let doc = "Prose\n<!-- aigarden:cog sh \"x\" -->\n```text\ncode\n```\n<!-- aigarden:end -->\nAfter\n";
        let mut html = String::new();
        pulldown_cmark::html::push_html(&mut html, pulldown_cmark::Parser::new(doc));
        assert_eq!(
            html,
            "<p>Prose</p>\n<!-- aigarden:cog sh \"x\" -->\n\
             <pre><code class=\"language-text\">code\n</code></pre>\n\
             <!-- aigarden:end -->\n<p>After</p>\n"
        );
    }

    #[test]
    fn char_pos_maps_first_column_of_each_line() {
        let content = "abc\ndef\n";
        // (line 1, col 1) is byte 0; (line 2, col 1) is byte 4 (after "abc\n").
        assert_eq!(char_pos_to_byte(content, 1, 1), 0);
        assert_eq!(char_pos_to_byte(content, 2, 1), 4);
    }

    #[test]
    fn char_pos_counts_multibyte_chars_not_bytes() {
        // "é" is two UTF-8 bytes; column 2 (the 'x') must land on byte 2, not 1.
        let content = "éx\n";
        assert_eq!(char_pos_to_byte(content, 1, 2), 2);
    }

    #[test]
    fn char_pos_clamps_an_overlong_column_to_line_end() {
        let content = "ab\ncd\n";
        // Column 99 on line 1 clamps to the end of "ab" (byte 2).
        assert_eq!(char_pos_to_byte(content, 1, 99), 2);
    }
}
