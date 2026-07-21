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
    #[expect(
        dead_code,
        reason = "consumed by rules needing the on-disk path, e.g. WS2 link rules"
    )]
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

    let mut files = Vec::new();
    for entry in builder.build() {
        let entry = entry.context("walking the file tree")?;
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        let abs_path = entry.path();
        let rel_path = display_path(abs_path, cwd);
        if exclude_set.is_match(&rel_path) {
            continue;
        }
        let bytes =
            std::fs::read(abs_path).with_context(|| format!("reading {}", abs_path.display()))?;
        files.push(SourceFile {
            rel_path,
            abs_path: abs_path.to_path_buf(),
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

fn build_glob_set(patterns: &[String]) -> Result<GlobSet> {
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        builder.add(Glob::new(pattern).with_context(|| format!("invalid glob {pattern:?}"))?);
    }
    Ok(builder.build()?)
}
