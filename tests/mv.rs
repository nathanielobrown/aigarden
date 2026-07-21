//! End-to-end snapshots and assertions for `ailint mv`: reference rewrites, the
//! re-anchor of the moved file's own links, and the git staging of the whole move.

use std::fs;
use std::path::Path;
use std::process::Command;

use insta_cmd::assert_cmd_snapshot;

mod common;
use common::{ailint, write};

/// Run `git <args>` in `dir`, asserting success.
fn git(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap();
    assert!(status.status.success(), "git {args:?} failed");
}

/// Return `git status --porcelain --no-renames` output for `dir`. `--no-renames`
/// keeps it deterministic: a git-mv'd file shows as a staged delete + staged add
/// rather than a rename, independent of the caller's diff.renames setting.
fn git_status(dir: &Path) -> String {
    let out = Command::new("git")
        .current_dir(dir)
        .args(["status", "--porcelain", "--no-renames"])
        .output()
        .unwrap();
    assert!(out.status.success());
    String::from_utf8(out.stdout).unwrap()
}

#[test]
fn mv_rewrites_references_across_the_repo() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "guide.md", "# Guide\n");
    write(
        dir.path(),
        "docs/readme.md",
        "See [the guide](../guide.md).\n",
    );
    // Move the guide into docs/; the referrer's link must repoint.
    assert_cmd_snapshot!(ailint(dir.path()).args(["mv", "guide.md", "docs/guide.md"]));
    insta::assert_snapshot!(
        "mv_rewrites_referrer",
        fs::read_to_string(dir.path().join("docs/readme.md")).unwrap()
    );
}

#[test]
fn mv_reanchors_the_moved_files_own_links() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "sibling.md", "# Sibling\n");
    write(dir.path(), "doc.md", "Link to [sib](sibling.md).\n");
    // Moving doc.md into sub/ must re-anchor its own outbound link.
    assert_cmd_snapshot!(ailint(dir.path()).args(["mv", "doc.md", "sub/doc.md"]));
    insta::assert_snapshot!(
        "mv_reanchored_moved_file",
        fs::read_to_string(dir.path().join("sub/doc.md")).unwrap()
    );
}

#[test]
fn mv_into_a_directory_uses_the_source_filename() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "notes.md", "# Notes\n");
    // A trailing-slash destination is a directory to move into.
    assert_cmd_snapshot!(ailint(dir.path()).args(["mv", "notes.md", "archive/"]));
    assert!(dir.path().join("archive/notes.md").exists());
}

#[test]
fn mv_refuses_when_the_destination_exists() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "a.md", "# A\n");
    write(dir.path(), "b.md", "# B\n");
    assert_cmd_snapshot!(ailint(dir.path()).args(["mv", "a.md", "b.md"]));
}

#[test]
fn mv_rewrites_both_a_file_relative_link_and_a_root_relative_bare_path() {
    // The check/mv drift repro: a subdir doc cites the SAME target two ways — a
    // file-relative markdown link AND a root-relative backticked bare path. The
    // bare-path *check* resolves root-relative, so mv's rewrite must too; otherwise
    // it repoints the link, leaves the backtick stale, and its own verify-after
    // (which runs bare-path) fails with exit 1. Both forms must rewrite and the
    // move must land clean (exit 0).
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "docs/persistence.md", "# Persistence\n");
    write(
        dir.path(),
        "plans/phase.md",
        "See the storage map at [map](../docs/persistence.md) — root form `docs/persistence.md`.\n",
    );
    assert_cmd_snapshot!(ailint(dir.path()).args([
        "mv",
        "docs/persistence.md",
        "docs/storage/persistence.md"
    ]));
    insta::assert_snapshot!(
        "mv_rewrites_root_relative_bare_path",
        fs::read_to_string(dir.path().join("plans/phase.md")).unwrap()
    );
}

#[test]
fn mv_stages_the_rename_and_its_rewrites_but_not_unrelated_changes() {
    // In a real git repo, `git mv` stages the rename; `mv` must likewise stage the
    // reference rewrites it makes to OTHER files, so the whole move lands as one
    // fully-staged changeset (no downstream `git add` needed). Scoping is the whole
    // point: an unrelated dirty file the user is editing must be left UNSTAGED.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q"]);
    git(root, &["config", "user.email", "t@example.com"]);
    git(root, &["config", "user.name", "Test"]);
    write(root, "guide.md", "# Guide\n");
    write(root, "docs/readme.md", "See [the guide](../guide.md).\n");
    write(root, "unrelated.md", "# Unrelated\noriginal line\n");
    git(root, &["add", "-A"]);
    git(root, &["commit", "-qm", "base"]);
    // A dirty, UNSTAGED local edit ailint must not sweep into the move.
    write(root, "unrelated.md", "# Unrelated\nlocally edited\n");

    let out = ailint(root)
        .args(["mv", "guide.md", "docs/guide.md"])
        .output()
        .unwrap();
    assert!(out.status.success(), "mv exited non-zero: {out:?}");

    let status = git_status(root);
    let lines: Vec<&str> = status.lines().collect();
    // The rename is staged (delete of old path + add of new, both in the index).
    assert!(lines.contains(&"A  docs/guide.md"), "status was:\n{status}");
    assert!(lines.contains(&"D  guide.md"), "status was:\n{status}");
    // The rewritten referrer is staged — index-modified, clean worktree column.
    assert!(
        lines.contains(&"M  docs/readme.md"),
        "rewrite left unstaged; status was:\n{status}"
    );
    // The unrelated edit is left UNSTAGED (worktree column M, index column blank).
    assert!(
        lines.contains(&" M unrelated.md"),
        "unrelated file changed staging; status was:\n{status}"
    );
    // Nothing the move touched is left with a dirty worktree column: the only
    // unstaged entry in the repo is the unrelated file.
    for line in &lines {
        let worktree = line.as_bytes()[1] as char;
        if worktree != ' ' {
            assert_eq!(*line, " M unrelated.md", "unexpected unstaged entry");
        }
    }
}
