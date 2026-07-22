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

use rumdl_lib::config::MarkdownFlavor;
use rumdl_lib::rule::{LintWarning, Rule as RumdlRule};
use rumdl_lib::rules::{
    MD009TrailingSpaces, MD010NoHardTabs, MD012NoMultipleBlanks, MD013Config, MD013LineLength,
    MD047SingleTrailingNewline, MD051LinkFragments,
};

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

/// The curated auto-fixable style set. These are the universally-agreed markdown
/// hygiene rules (trailing spaces, hard tabs, blank-line runs, final newline);
/// `reflow` adds paragraph re-wrapping (MD013) when the caller opts in.
pub(crate) fn style_rules(reflow: bool) -> Vec<Box<dyn RumdlRule>> {
    let mut rules: Vec<Box<dyn RumdlRule>> = vec![
        Box::new(MD009TrailingSpaces::new(2, false)),
        Box::new(MD010NoHardTabs::new(4)),
        Box::new(MD012NoMultipleBlanks::new(1)),
        Box::new(MD047SingleTrailingNewline),
    ];
    if reflow {
        // reflow=true re-wraps over-length paragraphs to the line-length limit.
        rules.push(Box::new(MD013LineLength::from_config_struct(MD013Config {
            reflow: true,
            ..MD013Config::default()
        })));
    }
    rules
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
