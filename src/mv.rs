//! `ailint mv <src> <dst>`: move a file and rewrite every reference to it, then
//! verify the repository is still clean.
//!
//! The rewrite reads from the same [`crate::references`] extraction core the link
//! rules audit, so `mv` can never drift from `check` — the single grammar is the
//! whole design. Two rewrites happen in one pass:
//!
//! - **Inbound** — every reference across the repo that pointed at `src` is
//!   repointed at `dst` (relative links recomputed from the referring file, code
//!   doc-refs kept root-relative), preserving any `#fragment`.
//! - **Outbound re-anchor** — the moved file's own relative links were written
//!   from `src`'s directory; they are recomputed from `dst`'s directory so they
//!   still resolve.
//!
//! After rewriting, the reference-integrity rules re-run over the repo and report
//! anything still broken — `mv` treats its own output as unverified until the
//! link rules say it is clean (the source tool's verify-after step).

use std::env;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use anyhow::{Context, Result, bail};

use crate::cli::{Cli, OutputFormat};
use crate::config::Config;
use crate::references::{RefKind, extract};
use crate::rules::resolve::{
    is_checkable_local, normalize_lexical, relative_path, resolve_from_file, resolve_from_root,
};
use crate::walk::{self, SourceFile};
use crate::{engine, output};

/// The reference-integrity rules `mv` re-runs to verify it left no broken links.
const LINK_RULES: [&str; 6] = [
    "link-target",
    "link-case",
    "bare-path",
    "import-target",
    "code-doc-ref",
    "anchor-resolves",
];

/// Move `src` to `dst`, rewrite every reference, and verify. Exit 0 when the repo
/// is clean afterward, 1 when the verify step still finds broken references, 2 on
/// a move or IO error.
pub(crate) fn run(cli: &Cli, src: &str, dst: &str, out: &mut impl Write) -> Result<ExitCode> {
    let cwd = env::current_dir()?;
    let loaded = Config::discover(cli.config.as_deref(), &cwd)?;

    let src_abs = resolve_from_root(&cwd, src);
    if !src_abs.exists() {
        bail!("source `{src}` does not exist");
    }
    if src_abs.is_dir() {
        bail!("`{src}` is a directory; `ailint mv` moves files only (for now)");
    }
    let dst_abs = destination(&cwd, &src_abs, dst);
    if dst_abs.exists() {
        bail!("destination `{}` already exists", display(&dst_abs, &cwd));
    }

    move_file(&src_abs, &dst_abs)?;

    // Walk the tree as it stands after the move (dst present, src gone).
    let files = walk::walk(
        &[PathBuf::from(".")],
        &loaded.config.exclude.effective_paths(),
        &cwd,
    )?;
    let (files_touched, refs_rewritten) = rewrite_references(&files, &cwd, &src_abs, &dst_abs)?;

    if matches!(cli.output_format, OutputFormat::Human) {
        writeln!(
            out,
            "moved {} \u{2192} {} (rewrote {refs_rewritten} reference(s) across {files_touched} file(s))",
            display(&src_abs, &cwd),
            display(&dst_abs, &cwd),
        )?;
    }

    // Verify-after: re-read the tree and re-run only the reference-integrity rules.
    let verified = walk::walk(
        &[PathBuf::from(".")],
        &loaded.config.exclude.effective_paths(),
        &cwd,
    )?;
    let residue = engine::check_with(&verified, &loaded.config, &cwd, |name| {
        LINK_RULES.contains(&name)
    })?;
    let sources = output::sources_from(&verified);
    output::render(cli.output_format, &residue, verified.len(), &sources, out)?;
    Ok(if residue.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    })
}

/// The absolute destination path: `dst` verbatim, or, when `dst` names a
/// directory (trailing slash or an existing dir), `dst`/`<src filename>`.
fn destination(cwd: &Path, src_abs: &Path, dst: &str) -> PathBuf {
    let dst_abs = resolve_from_root(cwd, dst);
    let into_dir =
        dst.ends_with('/') || dst.ends_with(std::path::MAIN_SEPARATOR) || dst_abs.is_dir();
    if into_dir {
        let name = src_abs.file_name().expect("source has a file name");
        dst_abs.join(name)
    } else {
        dst_abs
    }
}

/// Move the file with `git mv` when it is tracked, else a plain rename. Either
/// way the destination's parent directory is created first.
fn move_file(src_abs: &Path, dst_abs: &Path) -> Result<()> {
    if let Some(parent) = dst_abs.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    if is_git_tracked(src_abs) {
        let status = Command::new("git")
            .arg("mv")
            .arg(src_abs)
            .arg(dst_abs)
            .output()
            .context("running `git mv`")?;
        if !status.status.success() {
            bail!(
                "`git mv` failed: {}",
                String::from_utf8_lossy(&status.stderr).trim()
            );
        }
    } else {
        std::fs::rename(src_abs, dst_abs)
            .with_context(|| format!("renaming {} to {}", src_abs.display(), dst_abs.display()))?;
    }
    Ok(())
}

/// Whether `git` tracks `path` (so `git mv` preserves its history). A false here
/// (untracked file, or no git at all) falls back to a plain rename, which never
/// loses data.
fn is_git_tracked(path: &Path) -> bool {
    Command::new("git")
        .arg("ls-files")
        .arg("--error-unmatch")
        .arg(path)
        .output()
        .is_ok_and(|o| o.status.success())
}

/// Rewrite every reference to the moved file and re-anchor the moved file's own
/// relative links, writing each changed file back. Returns `(files_touched,
/// refs_rewritten)`.
fn rewrite_references(
    files: &[SourceFile],
    root: &Path,
    src_abs: &Path,
    dst_abs: &Path,
) -> Result<(usize, usize)> {
    let dst_dir = dst_abs.parent().unwrap_or(dst_abs);
    let mut files_touched = 0;
    let mut refs_rewritten = 0;
    for file in files {
        let file_abs = normalize_lexical(&file.abs_path);
        let is_moved = file_abs == *dst_abs;
        let current_dir = file_abs.parent().unwrap_or(&file_abs);
        let mut edits: Vec<(std::ops::Range<usize>, String)> = Vec::new();
        for reference in extract(&file.rel_path, &file.content) {
            let Some(path) = reference.path.as_deref().filter(|p| is_checkable_local(p)) else {
                continue;
            };
            let fragment = reference
                .fragment
                .as_deref()
                .map_or_else(String::new, |f| format!("#{f}"));
            let is_root_relative = reference.kind == RefKind::CodeDocRef;
            // Resolve the reference to an absolute target. For the moved file, its
            // relative links were written from src's old directory.
            let resolved = if is_root_relative {
                resolve_from_root(root, path)
            } else if is_moved {
                resolve_from_file(src_abs, path)
            } else {
                resolve_from_file(&file_abs, path)
            };
            let new_target = if resolved == *src_abs {
                // Inbound: this reference pointed at the moved file — repoint it.
                let new_path = if is_root_relative {
                    relative_path(root, dst_abs)
                } else if is_moved {
                    // A self-reference in the moved file.
                    relative_path(dst_dir, dst_abs)
                } else {
                    relative_path(current_dir, dst_abs)
                };
                format!("{new_path}{fragment}")
            } else if is_moved && !is_root_relative {
                // Outbound: re-anchor the moved file's other relative links.
                let new_path = relative_path(dst_dir, &resolved);
                format!("{new_path}{fragment}")
            } else {
                continue;
            };
            if new_target != reference.raw_target {
                edits.push((reference.target_span, new_target));
            }
        }
        if edits.is_empty() {
            continue;
        }
        // Apply back-to-front so earlier spans stay valid.
        edits.sort_by_key(|(span, _)| std::cmp::Reverse(span.start));
        let mut content = file.content.clone();
        for (span, replacement) in &edits {
            content.replace_range(span.clone(), replacement);
        }
        std::fs::write(&file.abs_path, &content)
            .with_context(|| format!("writing rewritten {}", file.rel_path))?;
        files_touched += 1;
        refs_rewritten += edits.len();
    }
    Ok((files_touched, refs_rewritten))
}

/// A path shown relative to `cwd` when possible, forward-slashed, for messages.
fn display(path: &Path, cwd: &Path) -> String {
    path.strip_prefix(cwd)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}
