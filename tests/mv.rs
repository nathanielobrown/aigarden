//! End-to-end snapshots and assertions for `aigarden mv`: reference rewrites, the
//! re-anchor of the moved file's own links, and the git staging of the whole move.

use std::fs;
use std::path::Path;
use std::process::Command;

use insta_cmd::assert_cmd_snapshot;

mod common;
use common::{aigarden, write};

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
    assert_cmd_snapshot!(aigarden(dir.path()).args(["mv", "guide.md", "docs/guide.md"]));
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
    assert_cmd_snapshot!(aigarden(dir.path()).args(["mv", "doc.md", "sub/doc.md"]));
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
    assert_cmd_snapshot!(aigarden(dir.path()).args(["mv", "notes.md", "archive/"]));
    assert!(dir.path().join("archive/notes.md").exists());
}

#[test]
fn mv_refuses_when_the_destination_exists() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "a.md", "# A\n");
    write(dir.path(), "b.md", "# B\n");
    assert_cmd_snapshot!(aigarden(dir.path()).args(["mv", "a.md", "b.md"]));
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
    assert_cmd_snapshot!(aigarden(dir.path()).args([
        "mv",
        "docs/persistence.md",
        "docs/storage/persistence.md"
    ]));
    insta::assert_snapshot!(
        "mv_rewrites_root_relative_bare_path",
        fs::read_to_string(dir.path().join("plans/phase.md")).unwrap()
    );
}

/// A `[status-header]` contract whose terminal docs are *fully* frozen: the link
/// and bare-path rules are both suppressed, so nothing checks their citations.
const FROZEN_LINKS_CONFIG: &str = "\
[status-header]
files = [\"plans/*.md\"]
live = [\"active\"]
terminal = [\"implemented\"]
suppresses = [\"link-target\", \"bare-path\"]
";

#[test]
fn mv_leaves_a_frozen_docs_citations_alone_when_the_exemption_covers_them() {
    // A shipped plan is as-built history. With its citations exempt from checking,
    // a rename has no reason to edit it — and a repo that freezes terminal docs at
    // commit time cannot afford the edit. The live plan cites the same file two
    // ways and is rewritten as usual; the frozen one comes out byte-identical, and
    // the move still exits 0 because the residue it leaves is unchecked.
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "aigarden.toml", FROZEN_LINKS_CONFIG);
    write(dir.path(), "docs/persistence.md", "# Persistence\n");
    let frozen = "# Shipped\n\n**Status:** implemented (2026-01-01)\n\n\
        Built against [the map](../docs/persistence.md) — root form `docs/persistence.md`.\n";
    write(dir.path(), "plans/shipped.md", frozen);
    write(
        dir.path(),
        "plans/live.md",
        "# Live\n\n**Status:** active\n\nSee [the map](../docs/persistence.md).\n",
    );
    assert_cmd_snapshot!(aigarden(dir.path()).args([
        "mv",
        "docs/persistence.md",
        "docs/storage/persistence.md"
    ]));
    assert_eq!(
        fs::read_to_string(dir.path().join("plans/shipped.md")).unwrap(),
        frozen,
        "the frozen plan was edited"
    );
    insta::assert_snapshot!(
        "mv_rewrites_the_live_plan_beside_a_frozen_one",
        fs::read_to_string(dir.path().join("plans/live.md")).unwrap()
    );
}

#[test]
fn mv_leaves_a_frozen_directorys_siblings_alone() {
    // When the tracked item is a directory (`inherits-from`), the shipped plan.md
    // freezes the grill record beside it: nothing checks that record's citations,
    // so the rename skips it exactly as it skips the plan. The live directory's
    // sibling is working material and is rewritten like any other file.
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "aigarden.toml",
        "[status-header]\nfiles = [\"plans/*.md\"]\nlive = [\"active\"]\n\
         terminal = [\"implemented\"]\ninherits-from = \"plan.md\"\nsuppresses = [\"link-target\"]\n",
    );
    write(dir.path(), "docs/persistence.md", "# Persistence\n");
    write(
        dir.path(),
        "plans/shipped/plan.md",
        "# Shipped\n\n**Status:** implemented (2026-01-01)\n",
    );
    let frozen_sibling = "# Grill\n\nWeighed [the map](../../docs/persistence.md).\n";
    write(dir.path(), "plans/shipped/grill.md", frozen_sibling);
    write(
        dir.path(),
        "plans/building/plan.md",
        "# Building\n\n**Status:** active\n",
    );
    write(
        dir.path(),
        "plans/building/grill.md",
        "# Grill\n\nWeighing [the map](../../docs/persistence.md).\n",
    );
    assert_cmd_snapshot!(aigarden(dir.path()).args([
        "mv",
        "docs/persistence.md",
        "docs/storage/persistence.md"
    ]));
    assert_eq!(
        fs::read_to_string(dir.path().join("plans/shipped/grill.md")).unwrap(),
        frozen_sibling,
        "the frozen directory's sibling was edited"
    );
    insta::assert_snapshot!(
        "mv_rewrites_a_live_directorys_sibling",
        fs::read_to_string(dir.path().join("plans/building/grill.md")).unwrap()
    );
}

#[test]
fn mv_rewrites_a_frozen_doc_whose_citations_are_still_checked() {
    // The skip is keyed on the exemption, not on frozenness: with an empty
    // `suppresses`, the frozen plan's link is still checked, so leaving it stale
    // would break the repo — mv rewrites it exactly as it always did.
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "aigarden.toml",
        "[status-header]\nfiles = [\"plans/*.md\"]\nlive = [\"active\"]\nterminal = [\"implemented\"]\n",
    );
    write(dir.path(), "docs/persistence.md", "# Persistence\n");
    write(
        dir.path(),
        "plans/shipped.md",
        "# Shipped\n\n**Status:** implemented (2026-01-01)\n\nSee [the map](../docs/persistence.md).\n",
    );
    assert_cmd_snapshot!(aigarden(dir.path()).args([
        "mv",
        "docs/persistence.md",
        "docs/storage/persistence.md"
    ]));
    insta::assert_snapshot!(
        "mv_rewrites_a_checked_frozen_doc",
        fs::read_to_string(dir.path().join("plans/shipped.md")).unwrap()
    );
}

#[test]
fn mv_reanchors_a_frozen_doc_it_moves_itself() {
    // The one edit a frozen doc still gets: moving it *is* an edit to it, so its
    // own outbound links are re-anchored rather than left dangling. The skip
    // protects frozen docs from a rename of some *other* file.
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "aigarden.toml", FROZEN_LINKS_CONFIG);
    write(dir.path(), "docs/persistence.md", "# Persistence\n");
    write(
        dir.path(),
        "plans/shipped.md",
        "# Shipped\n\n**Status:** implemented (2026-01-01)\n\nSee [the map](../docs/persistence.md).\n",
    );
    assert_cmd_snapshot!(aigarden(dir.path()).args([
        "mv",
        "plans/shipped.md",
        "plans/archive/shipped.md"
    ]));
    insta::assert_snapshot!(
        "mv_reanchored_moved_frozen_doc",
        fs::read_to_string(dir.path().join("plans/archive/shipped.md")).unwrap()
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
    // A dirty, UNSTAGED local edit aigarden must not sweep into the move.
    write(root, "unrelated.md", "# Unrelated\nlocally edited\n");

    let out = aigarden(root)
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
