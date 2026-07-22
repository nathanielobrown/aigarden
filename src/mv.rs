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
    is_checkable_local, normalize_lexical, relative_path, resolve_existing, resolve_from_file,
    resolve_from_root,
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

    // The move and its rewrites either both go through git (src tracked) or both
    // stay off it (plain rename). Decide once so staging mirrors `git mv`: when
    // `git mv` stages the rename, the reference rewrites are staged too, so the
    // whole move lands as one changeset rather than leaving rewrites for a
    // downstream `git add` to sweep (which risks staging unrelated worktree edits).
    let git_move = is_git_tracked(&src_abs);
    move_file(&src_abs, &dst_abs, git_move)?;

    // Walk the tree as it stands after the move (dst present, src gone).
    let files = walk::walk(
        &[PathBuf::from(".")],
        &loaded.config.effective_excludes(),
        &cwd,
    )?;
    let (files_touched, refs_rewritten) =
        rewrite_references(&files, &cwd, &src_abs, &dst_abs, git_move)?;

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
        &loaded.config.effective_excludes(),
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

/// Move the file with `git mv` when `git_move` (src is tracked), else a plain
/// rename. Either way the destination's parent directory is created first.
fn move_file(src_abs: &Path, dst_abs: &Path, git_move: bool) -> Result<()> {
    if let Some(parent) = dst_abs.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    if git_move {
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
    git_move: bool,
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
            // Resolve the reference to an absolute target, and record which style it
            // was written in so the rewrite can preserve it. Most kinds are fixed:
            // code doc-refs are always root-relative, links/images/imports always
            // file-relative (from src's old dir for the moved file). A bare path is
            // the exception — the `bare-path` check accepts either resolution, so the
            // rewrite must mirror it, or a root-relative backtick in a subdir doc is
            // left stale and the verify-after fails (the check/mv drift this fixes).
            let base_for_file: &Path = if is_moved { src_abs } else { &file_abs };
            let (resolved, is_root_relative) = match reference.kind {
                RefKind::CodeDocRef => (resolve_from_root(root, path), true),
                RefKind::BarePath => {
                    let from_file = resolve_from_file(base_for_file, path);
                    let from_root = resolve_from_root(root, path);
                    // An inbound match picks the matching style; otherwise prefer the
                    // style that resolves on disk (file-relative wins ties), so an
                    // outbound bare path in the moved file re-anchors correctly and a
                    // still-valid root-relative one is left untouched.
                    if from_file == *src_abs {
                        (from_file, false)
                    } else if from_root == *src_abs {
                        (from_root, true)
                    } else if resolve_existing(&from_file).is_some() {
                        (from_file, false)
                    } else {
                        (from_root, true)
                    }
                }
                _ => (resolve_from_file(base_for_file, path), false),
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
        // Stage exactly this rewrite when the move went through git, so the whole
        // change (rename + every referencing edit) is one staged changeset. Only
        // the file ailint just wrote is staged — never anything else in the tree.
        if git_move {
            git_add(&file.abs_path)?;
        }
        files_touched += 1;
        refs_rewritten += edits.len();
    }
    Ok((files_touched, refs_rewritten))
}

/// `git add` a single path — used to stage each file `mv` rewrote, matching the
/// staging `git mv` already does for the rename.
fn git_add(path: &Path) -> Result<()> {
    let status = Command::new("git")
        .arg("add")
        .arg(path)
        .output()
        .context("running `git add`")?;
    if !status.status.success() {
        bail!(
            "`git add {}` failed: {}",
            path.display(),
            String::from_utf8_lossy(&status.stderr).trim()
        );
    }
    Ok(())
}

/// A path shown relative to `cwd` when possible, forward-slashed, for messages.
fn display(path: &Path, cwd: &Path) -> String {
    path.strip_prefix(cwd)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}
