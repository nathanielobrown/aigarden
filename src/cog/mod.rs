//! The cog engine: find generated regions, regenerate them, and either gate
//! their freshness (`--check` / the `cog-fresh` rule) or splice updates in place
//! (`--write`).
//!
//! A cog block is an HTML-comment marker pair delimiting a machine-owned region:
//!
//! ```text
//! <!-- aigarden:cog file-tree src -->
//! …generated body, recomputed on every run…
//! <!-- aigarden:end -->
//! ```
//!
//! The markers are HTML comments so they vanish in rendered markdown. The open
//! marker names a generator (a built-in, or `sh` with an embedded command); the
//! body between the markers belongs to the tool. Parsing is fence-aware: a marker
//! inside a fenced code block is a documented example, not a live block, so a doc
//! can show the marker syntax without expanding it.

use std::io::Write;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Result, bail};

use crate::cli::OutputFormat;
use crate::diagnostic::{Diagnostic, Span};
use crate::output;
use crate::references::is_markdown;
use crate::walk::SourceFile;

mod generators;

/// A parsed cog block: the generator directive plus the byte spans of the open
/// marker line (for diagnostics) and the machine-owned body (for splicing).
pub(crate) struct CogBlock {
    generator: String,
    args: String,
    /// Byte span of the open marker line, without its trailing newline.
    open_marker_span: Range<usize>,
    /// Byte span of the body between the markers (may be empty).
    body_span: Range<usize>,
}

/// One freshness finding: a stale block, or a generator that failed to run.
pub(crate) struct CogFinding {
    pub(crate) open_marker_span: Range<usize>,
    pub(crate) message: String,
    pub(crate) kind: FindingKind,
}

/// Whether a finding is a stale body or a generator failure. `--check` treats a
/// failure as a tool error (exit 2); the `cog-fresh` rule treats both as findings
/// so `aigarden check` never aborts on one bad generator.
#[derive(PartialEq, Eq)]
pub(crate) enum FindingKind {
    Stale,
    Failed,
}

/// `aigarden cog --check`: gate every markdown file's cog blocks. A failing
/// generator is a tool error (exit 2, loud on stderr); a stale block is a
/// `cog-fresh` diagnostic (exit 1); a fresh repo is quiet (exit 0).
pub(crate) fn check_repo(
    format: OutputFormat,
    files: &[SourceFile],
    cwd: &Path,
    out: &mut impl Write,
) -> Result<ExitCode> {
    let mut diagnostics = Vec::new();
    let mut failures = Vec::new();
    for file in files.iter().filter(|f| is_markdown(&f.rel_path)) {
        let root = repo_root(file.abs_path.parent().unwrap_or(&file.abs_path), cwd);
        for finding in evaluate(&file.content, &file.abs_path, &root) {
            match finding.kind {
                FindingKind::Failed => {
                    failures.push(format!("{}: {}", file.rel_path, finding.message));
                }
                FindingKind::Stale => {
                    diagnostics.push(to_diagnostic(&file.rel_path, &file.content, &finding));
                }
            }
        }
    }
    if !failures.is_empty() {
        bail!(failures.join("\n"));
    }
    let sources = output::sources_from(files);
    output::render(format, &diagnostics, files.len(), &sources, out)?;
    Ok(if diagnostics.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    })
}

/// `aigarden cog --write`: regenerate every markdown file's cog blocks in place,
/// reporting which files changed. A failing generator aborts (exit 2) — a write
/// must be correct or not happen.
pub(crate) fn write_repo(
    files: &[SourceFile],
    cwd: &Path,
    out: &mut impl Write,
) -> Result<ExitCode> {
    let mut changed = 0;
    for file in files.iter().filter(|f| is_markdown(&f.rel_path)) {
        let root = repo_root(file.abs_path.parent().unwrap_or(&file.abs_path), cwd);
        if let Some(new_content) = rewrite(&file.content, &file.abs_path, &root)? {
            std::fs::write(&file.abs_path, &new_content)?;
            writeln!(out, "updated {}", file.rel_path)?;
            changed += 1;
        }
    }
    if changed == 0 {
        writeln!(out, "cog: all blocks already fresh")?;
    }
    Ok(ExitCode::SUCCESS)
}

/// Evaluate every cog block in `content`, returning a finding for each stale or
/// failing block. `file_abs` is the cog file; `repo_root` is its git root (the
/// cwd for `sh`). A fresh, error-free file yields no findings.
pub(crate) fn evaluate(content: &str, file_abs: &Path, repo_root: &Path) -> Vec<CogFinding> {
    let blocks = match find_blocks(content) {
        Ok(blocks) => blocks,
        // A malformed block set (nested/unterminated) is itself a finding.
        Err(err) => {
            return vec![CogFinding {
                open_marker_span: 0..0,
                message: err.to_string(),
                kind: FindingKind::Failed,
            }];
        }
    };
    let mut findings = Vec::new();
    for block in &blocks {
        match generate(block, file_abs, repo_root) {
            Ok(expected) => {
                if content[block.body_span.clone()] != expected {
                    findings.push(CogFinding {
                        open_marker_span: block.open_marker_span.clone(),
                        message: format!(
                            "cog block `{}` is out of date \u{2014} run `aigarden cog --write`",
                            block.generator
                        ),
                        kind: FindingKind::Stale,
                    });
                }
            }
            Err(err) => findings.push(CogFinding {
                open_marker_span: block.open_marker_span.clone(),
                message: format!("cog generator `{}` failed: {err:#}", block.generator),
                kind: FindingKind::Failed,
            }),
        }
    }
    findings
}

/// Regenerate every cog block in `content`, returning the new file text, or
/// `None` when nothing changed. A generator error aborts the whole rewrite (a
/// write must be correct or not happen), unlike the per-block boundary in
/// [`evaluate`].
pub(crate) fn rewrite(content: &str, file_abs: &Path, repo_root: &Path) -> Result<Option<String>> {
    let blocks = find_blocks(content)?;
    // Splice bodies back-to-front so earlier spans stay valid as we edit.
    let mut out = content.to_string();
    let mut changed = false;
    for block in blocks.iter().rev() {
        let expected = generate(block, file_abs, repo_root)?;
        if content[block.body_span.clone()] != expected {
            out.replace_range(block.body_span.clone(), &expected);
            changed = true;
        }
    }
    Ok(changed.then_some(out))
}

/// Build a `cog-fresh` diagnostic from a finding against `file`'s content.
pub(crate) fn to_diagnostic(rel_path: &str, content: &str, finding: &CogFinding) -> Diagnostic {
    Diagnostic {
        rule: "cog-fresh",
        path: rel_path.to_string(),
        span: Some(Span::from_byte_range(
            content,
            finding.open_marker_span.clone(),
        )),
        message: finding.message.clone(),
        suggestion: None,
    }
}

/// The git root at or above `start`, else `fallback` — the cwd `sh` generators
/// run in. Walking up for `.git` means a cog file deep in the tree still resolves
/// commands against the repository root, not its own directory.
pub(crate) fn repo_root(start: &Path, fallback: &Path) -> PathBuf {
    let mut dir = start;
    loop {
        if dir.join(".git").exists() {
            return dir.to_path_buf();
        }
        match dir.parent() {
            Some(parent) => dir = parent,
            None => return fallback.to_path_buf(),
        }
    }
}

/// Run a block's generator, normalizing the output so the end marker always lands
/// on its own line: a non-empty body ends in exactly one newline.
fn generate(block: &CogBlock, file_abs: &Path, repo_root: &Path) -> Result<String> {
    let mut body = generators::generate(&block.generator, &block.args, file_abs, repo_root)?;
    if !body.is_empty() && !body.ends_with('\n') {
        body.push('\n');
    }
    Ok(body)
}

/// Parse every cog block, fence-aware. Fails loudly on a nested open marker or an
/// unterminated block — a malformed region must never silently pass as fresh.
fn find_blocks(content: &str) -> Result<Vec<CogBlock>> {
    let mut blocks = Vec::new();
    let mut in_fence = false;
    let mut open: Option<(String, String, Range<usize>, usize)> = None;
    let mut offset = 0;
    for line in content.split_inclusive('\n') {
        let line_start = offset;
        offset += line.len();
        let has_newline = line.ends_with('\n');
        let text = line.trim_end_matches('\n');
        let trimmed = text.trim();
        // A fence delimiter toggles example-mode; it is never a marker itself.
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        if open.is_some() && is_end_marker(trimmed) {
            let (generator, args, open_marker_span, body_start) = open.take().unwrap();
            blocks.push(CogBlock {
                generator,
                args,
                open_marker_span,
                body_span: body_start..line_start,
            });
        } else if let Some((generator, args)) = parse_open_marker(trimmed) {
            if open.is_some() {
                bail!("nested cog open marker before the previous block closed");
            }
            // The open marker line spans up to (not including) its newline; the
            // body starts after the newline.
            let marker_end = line_start + text.len();
            let body_start = if has_newline { offset } else { marker_end };
            open = Some((generator, args, line_start..marker_end, body_start));
        }
    }
    if open.is_some() {
        bail!("unterminated cog block (missing `<!-- aigarden:end -->`)");
    }
    Ok(blocks)
}

/// Parse an open marker line into `(generator, args)`, or `None` if it is not one.
fn parse_open_marker(trimmed: &str) -> Option<(String, String)> {
    let inner = trimmed.strip_prefix("<!--")?.strip_suffix("-->")?.trim();
    let rest = inner.strip_prefix("aigarden:cog")?;
    // Require a boundary so `aigarden:cogfoo` is not a marker.
    if !rest.starts_with(char::is_whitespace) {
        return None;
    }
    let rest = rest.trim();
    let (generator, args) = match rest.split_once(char::is_whitespace) {
        Some((g, a)) => (g.to_string(), a.trim().to_string()),
        None => (rest.to_string(), String::new()),
    };
    if generator.is_empty() {
        return None;
    }
    Some((generator, args))
}

/// Whether a line is the `<!-- aigarden:end -->` close marker.
fn is_end_marker(trimmed: &str) -> bool {
    trimmed
        .strip_prefix("<!--")
        .and_then(|s| s.strip_suffix("-->"))
        .map(str::trim)
        == Some("aigarden:end")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_a_block_and_its_body_span() {
        let content =
            "before\n<!-- aigarden:cog sh \"echo x\" -->\nold body\n<!-- aigarden:end -->\nafter\n";
        let blocks = find_blocks(content).unwrap();
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].generator, "sh");
        assert_eq!(blocks[0].args, "\"echo x\"");
        // The body is exactly the line between the markers, including its newline.
        assert_eq!(&content[blocks[0].body_span.clone()], "old body\n");
        // The open marker span slices back to the marker line.
        assert_eq!(
            &content[blocks[0].open_marker_span.clone()],
            "<!-- aigarden:cog sh \"echo x\" -->"
        );
    }

    #[test]
    fn markers_inside_a_fence_are_examples_not_blocks() {
        let content = "```\n<!-- aigarden:cog file-tree src -->\nx\n<!-- aigarden:end -->\n```\n";
        assert!(find_blocks(content).unwrap().is_empty());
    }

    #[test]
    fn an_unterminated_block_fails_loudly() {
        let content = "<!-- aigarden:cog index docs/*.md -->\nbody without end\n";
        assert!(find_blocks(content).is_err());
    }

    #[test]
    fn empty_body_between_adjacent_markers_is_valid() {
        let content = "<!-- aigarden:cog sh \"true\" -->\n<!-- aigarden:end -->\n";
        let blocks = find_blocks(content).unwrap();
        assert_eq!(blocks.len(), 1);
        assert_eq!(&content[blocks[0].body_span.clone()], "");
    }
}
