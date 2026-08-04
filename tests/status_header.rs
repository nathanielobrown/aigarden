//! The `status-header` contract end to end: the frozen set, the directory-item
//! form (`inherits-from`), the header requirement, and the two config errors that
//! keep a mistyped contract from silently doing nothing. Same shape as `cli.rs` —
//! the real binary against throwaway repos — split out because the family is big.

use insta_cmd::assert_cmd_snapshot;

mod common;
use common::{aigarden, write};

/// The `[status-header]` config a mycelia-shaped repo uses: issue/plan globs, a
/// live/terminal vocabulary, and the three rules the frozen exemption suppresses.
const STATUS_HEADER_CONFIG: &str = "\
[status-header]
files = [\"issues/**/*.md\", \"plans/*.md\"]
live = [\"open\"]
terminal = [\"done\", \"implemented\"]
suppresses = [\"bare-path\", \"link-case\", \"descriptive-anchor\"]
";

#[test]
fn frozen_terminal_doc_exempts_a_bare_path_a_live_doc_does_not() {
    // The core exemption: a terminal-status (`done`) issue may cite a now-gone path
    // as historical record, so bare-path skips it; the identical citation in a
    // live (`open`) issue is still flagged. Same missing target, opposite verdicts —
    // the exemption keys off the status header, not the path.
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "aigarden.toml", STATUS_HEADER_CONFIG);
    write(
        dir.path(),
        "issues/0001-closed.md",
        "# 0001: Historical\n\n**Status:** done\n\nOnce lived at `docs/gone.md`.\n",
    );
    write(
        dir.path(),
        "issues/0002-live.md",
        "# 0002: Live\n\n**Status:** open\n\nStill references `docs/gone.md`.\n",
    );
    assert_cmd_snapshot!(aigarden(dir.path()).arg("check"));
}

#[test]
fn frozen_doc_still_has_its_live_markdown_link_checked() {
    // The exemption is opt-in per rule: with link-target left out of `suppresses`, a
    // frozen doc's markdown link to a missing file is still a finding — the
    // historical-record allowance covers only the rules the repo named.
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "aigarden.toml", STATUS_HEADER_CONFIG);
    write(
        dir.path(),
        "issues/0003-closed.md",
        "# 0003: Closed\n\n**Status:** done\n\nSee [the guide](docs/gone.md).\n",
    );
    assert_cmd_snapshot!(aigarden(dir.path()).arg("check"));
}

/// The same contract with the link rules added to `suppresses` — the "frozen
/// means frozen" configuration, for a repo whose terminal docs are untouchable
/// history rather than live documentation.
const FROZEN_LINKS_CONFIG: &str = "\
[status-header]
files = [\"issues/**/*.md\", \"plans/*.md\"]
live = [\"open\"]
terminal = [\"done\", \"implemented\"]
suppresses = [\"bare-path\", \"link-case\", \"descriptive-anchor\", \"link-target\", \"anchor-resolves\"]
";

#[test]
fn suppressing_the_link_rules_frees_a_frozen_docs_dead_link_and_anchor() {
    // Configuring link-target/anchor-resolves as suppressible (a config that used
    // to be rejected outright) makes a frozen doc's dead link and dangling anchor
    // stop being findings — while the identical link in the live issue still is.
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "aigarden.toml", FROZEN_LINKS_CONFIG);
    write(
        dir.path(),
        "issues/0006-closed.md",
        "# 0006: Closed\n\n**Status:** done\n\nSee [the guide](docs/gone.md) and [below](#no-such-heading).\n",
    );
    write(
        dir.path(),
        "issues/0007-live.md",
        "# 0007: Live\n\n**Status:** open\n\nSee [the guide](docs/gone.md).\n",
    );
    assert_cmd_snapshot!(aigarden(dir.path()).arg("check"));
}

#[test]
fn status_header_flags_an_unrecognized_status() {
    // Fail loud, never a silent skip: a doc under the contract whose status is a
    // typo is reported (and, being non-terminal, is not treated as frozen).
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "aigarden.toml", STATUS_HEADER_CONFIG);
    write(
        dir.path(),
        "issues/0004-typo.md",
        "# 0004: Typo\n\n**Status:** dnoe\n",
    );
    assert_cmd_snapshot!(aigarden(dir.path()).arg("check"));
}

#[test]
fn status_header_flags_a_missing_header() {
    // A tracker doc with no status header at all is likewise reported — the index
    // and the frozen set would otherwise silently misjudge it.
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "aigarden.toml", STATUS_HEADER_CONFIG);
    write(
        dir.path(),
        "plans/no-status.md",
        "# A plan with no status line\n\nBody only.\n",
    );
    assert_cmd_snapshot!(aigarden(dir.path()).arg("check"));
}

/// A contract whose tracked item is a *directory*: `plans/<topic>/plan.md` carries
/// the status for the working artifacts beside it (`inherits-from`), the shape a
/// repo uses when a design and its notes are one item with one lifecycle.
const DIR_ITEM_CONFIG: &str = "\
[status-header]
files = [\"plans/*.md\"]
live = [\"active\"]
terminal = [\"implemented\"]
inherits-from = \"plan.md\"
suppresses = [\"link-target\"]
";

#[test]
fn a_sibling_inherits_its_directorys_status_and_freezes_with_it() {
    // The item is the directory, so a grill record beside a shipped `plan.md` is
    // history too: its dead link is exempt, and it needs no status header of its
    // own. Three controls in one run — beside a live `plan.md` the identical link
    // is still a finding; a sibling carrying its own status is judged by that
    // header, not by the directory; and a directory with no `plan.md` at all is
    // unchanged (its file is still missing a status).
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "aigarden.toml", DIR_ITEM_CONFIG);
    let rot = "See [the map](../../docs/gone.md).\n";
    write(
        dir.path(),
        "plans/shipped/plan.md",
        "# Shipped\n\n**Status:** implemented (2026-01-01)\n",
    );
    write(
        dir.path(),
        "plans/shipped/grill.md",
        &format!("# Grill\n\n{rot}"),
    );
    write(
        dir.path(),
        "plans/shipped/reopened.md",
        &format!("# Reopened\n\n**Status:** active\n\n{rot}"),
    );
    write(
        dir.path(),
        "plans/building/plan.md",
        "# Building\n\n**Status:** active\n",
    );
    write(
        dir.path(),
        "plans/building/grill.md",
        &format!("# Grill\n\n{rot}"),
    );
    write(
        dir.path(),
        "plans/loose/notes.md",
        "# Notes\n\nNo plan beside it.\n",
    );
    assert_cmd_snapshot!(aigarden(dir.path()).arg("check"));
}

#[test]
fn inherits_from_naming_a_path_is_a_config_error() {
    // The value is matched against a doc's basename, so a path would govern nothing
    // and quietly leave every directory item unfrozen — a loud load error instead.
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "aigarden.toml",
        "[status-header]\nfiles = [\"plans/*.md\"]\nterminal = [\"implemented\"]\ninherits-from = \"plans/plan.md\"\n",
    );
    write(
        dir.path(),
        "plans/a/plan.md",
        "# A\n\n**Status:** implemented\n",
    );
    insta::with_settings!({filters => vec![(r"\S*aigarden\.toml", "[CONFIG]")]}, {
        assert_cmd_snapshot!(aigarden(dir.path()).arg("check"));
    });
}

#[test]
fn readme_under_a_tracker_glob_is_not_a_tracked_item() {
    // A README matched by a `files` glob is prose about the tracker, not an item —
    // it needs no status header and is never flagged.
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "aigarden.toml", STATUS_HEADER_CONFIG);
    write(
        dir.path(),
        "issues/README.md",
        "# Issues\n\nHow the tracker works.\n",
    );
    write(
        dir.path(),
        "issues/0005-open.md",
        "# 0005: Live\n\n**Status:** open\n",
    );
    assert_cmd_snapshot!(aigarden(dir.path()).arg("check"));
}

#[test]
fn suppresses_naming_a_non_frozen_aware_rule_is_a_config_error() {
    // Only a rule that declares itself frozen-aware — the markdown citation rules —
    // can honor the exemption; naming a structural one (here file-length) would be a
    // silent no-op, so it is a loud config error (exit 2) at load, never accepted.
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "aigarden.toml",
        "[status-header]\nfiles = [\"issues/**/*.md\"]\nterminal = [\"done\"]\nsuppresses = [\"file-length\"]\n",
    );
    write(
        dir.path(),
        "issues/0001.md",
        "# 0001: X\n\n**Status:** done\n",
    );
    insta::with_settings!({filters => vec![(r"\S*aigarden\.toml", "[CONFIG]")]}, {
        assert_cmd_snapshot!(aigarden(dir.path()).arg("check"));
    });
}

#[test]
fn explain_covers_the_status_header_rule() {
    // The frozen-docs contract's full config surface — files, header, live,
    // terminal, suppresses — must surface through explain for a configuring repo.
    let dir = tempfile::tempdir().unwrap();
    assert_cmd_snapshot!(aigarden(dir.path()).args(["explain", "status-header"]));
}
