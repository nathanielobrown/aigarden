//! Gitignore-aware file collection into [`SourceFile`]s with content read once.
//!
//! Content is read eagerly (lossy UTF-8) so every rule shares one read and no
//! rule needs to handle IO errors mid-run. Config `exclude` globs are applied on
//! top of the `ignore` crate's `.gitignore` handling — the single exclusion seam.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use globset::{Glob, GlobSet, GlobSetBuilder};
use ignore::WalkBuilder;

/// One walked file: its display path, absolute path, and full text.
pub(crate) struct SourceFile {
    /// Path relative to the invocation dir, forward-slashed — used in diagnostics.
    pub(crate) rel_path: String,
    /// Absolute on-disk path, used by rules that resolve references from a file's
    /// own directory (link/import targets).
    pub(crate) abs_path: PathBuf,
    /// File contents, lossily decoded as UTF-8 (invalid bytes become U+FFFD).
    pub(crate) content: String,
}

/// Walk `paths` (gitignore-respecting), dropping config-excluded files, and read
/// each remaining file's content. Display paths are relative to `cwd`.
pub(crate) fn walk(paths: &[PathBuf], exclude: &[String], cwd: &Path) -> Result<Vec<SourceFile>> {
    let exclude_set = build_glob_set(exclude).context("compiling exclude globs")?;

    let (first, rest) = paths.split_first().expect("at least one path to walk");
    let mut builder = WalkBuilder::new(first);
    for path in rest {
        builder.add(path);
    }
    // Walk hidden files: guidance docs live under `.claude/` and dotfiles like
    // `.cursorrules` carry @-imports the link rules must see. `.gitignore` still
    // applies; only `.git` itself is pruned (it is never source content).
    builder.hidden(false);
    builder.filter_entry(|entry| entry.file_name() != ".git");

    let mut files = Vec::new();
    for entry in builder.build() {
        let entry = entry.context("walking the file tree")?;
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        let walked = entry.path();
        let rel_path = display_path(walked, cwd);
        if exclude_set.is_match(&rel_path) {
            continue;
        }
        // Make the path genuinely absolute (a `.` scan root yields relative paths).
        // Cross-file rules key a workspace index by this path and must agree with
        // how they resolve link targets — a relative key silently misses.
        let abs_path = cwd.join(walked);
        let bytes =
            std::fs::read(&abs_path).with_context(|| format!("reading {}", abs_path.display()))?;
        files.push(SourceFile {
            rel_path,
            abs_path,
            content: String::from_utf8_lossy(&bytes).into_owned(),
        });
    }
    Ok(files)
}

/// Path relative to `cwd` when possible, forward-slashed for stable matching,
/// with any leading `./` (from a `.` scan root) trimmed.
fn display_path(abs: &Path, cwd: &Path) -> String {
    let rel = abs.strip_prefix(cwd).unwrap_or(abs);
    let slashed = rel.to_string_lossy().replace('\\', "/");
    slashed.strip_prefix("./").unwrap_or(&slashed).to_string()
}

/// Compile `patterns` into a globset matched against repo-relative paths. Shared
/// by the walker (global excludes) and the engine (per-rule excludes).
pub(crate) fn build_glob_set(patterns: &[String]) -> Result<GlobSet> {
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        builder.add(Glob::new(pattern).with_context(|| format!("invalid glob {pattern:?}"))?);
    }
    Ok(builder.build()?)
}
