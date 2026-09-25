//! End-to-end snapshots for `aigarden cog` and the `cog-fresh` rule: which files
//! the cog engine owns, each generator's output, and the check/write exit codes.

use std::fs;

use insta_cmd::assert_cmd_snapshot;

mod common;
use common::{aigarden, write};

#[test]
fn cog_requires_a_mode() {
    // Neither --check nor --write: clap rejects it (no default mode).
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "doc.md", "# Doc\n");
    assert_cmd_snapshot!(aigarden(dir.path()).arg("cog"));
}

#[test]
fn cog_check_flags_a_stale_block() {
    let dir = tempfile::tempdir().unwrap();
    // The body says `stale` but the generator produces `hello`.
    write(
        dir.path(),
        "doc.md",
        "# Doc\n\n<!-- aigarden:cog sh \"echo hello\" -->\nstale\n<!-- aigarden:end -->\n",
    );
    assert_cmd_snapshot!(aigarden(dir.path()).args(["cog", "--check"]));
}

#[test]
fn cog_write_regenerates_then_check_is_clean() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "doc.md",
        "# Doc\n\n<!-- aigarden:cog sh \"echo hello\" -->\nstale\n<!-- aigarden:end -->\n",
    );
    // --write splices the fresh body and reports the changed file.
    assert_cmd_snapshot!(aigarden(dir.path()).args(["cog", "--write"]));
    let written = fs::read_to_string(dir.path().join("doc.md")).unwrap();
    insta::assert_snapshot!("cog_write_file_contents", written);
    // A --check right after --write is always clean (determinism).
    assert_cmd_snapshot!(aigarden(dir.path()).args(["cog", "--check"]));
}

#[test]
fn cog_failing_generator_is_a_tool_error() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "doc.md",
        "<!-- aigarden:cog sh \"exit 3\" -->\n<!-- aigarden:end -->\n",
    );
    // A nonzero shell exit is exit 2 (tool error), not a finding.
    assert_cmd_snapshot!(aigarden(dir.path()).args(["cog", "--check"]));
}

#[test]
fn cog_file_tree_renders_a_directory_tree() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "src/a.rs", "fn a() {}\n");
    write(dir.path(), "src/nested/b.rs", "fn b() {}\n");
    write(dir.path(), "src/c.rs", "fn c() {}\n");
    write(
        dir.path(),
        "doc.md",
        "<!-- aigarden:cog file-tree src -->\n<!-- aigarden:end -->\n",
    );
    assert_cmd_snapshot!(aigarden(dir.path()).args(["cog", "--write"]));
    insta::assert_snapshot!(
        "cog_file_tree_contents",
        fs::read_to_string(dir.path().join("doc.md")).unwrap()
    );
}

#[test]
fn cog_first_sentences_projects_the_glossary() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "CONTEXT.md",
        "# Long form\n\n## Terms\n\n- **Alpha** \u{2014} the first thing. It has more prose.\n- **Beta** \u{2014} the second thing, with a `dotted.name` inside.\n",
    );
    write(
        dir.path(),
        "SHORT.md",
        "<!-- aigarden:cog first-sentences CONTEXT.md -->\n<!-- aigarden:end -->\n",
    );
    assert_cmd_snapshot!(aigarden(dir.path()).args(["cog", "--write"]));
    insta::assert_snapshot!(
        "cog_first_sentences_contents",
        fs::read_to_string(dir.path().join("SHORT.md")).unwrap()
    );
}

#[test]
fn cog_index_lists_matching_files_with_glosses() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "docs/0001-first.md",
        "# First decision\n\nWhy we chose the first thing. More detail.\n",
    );
    write(
        dir.path(),
        "docs/0002-second.md",
        "---\ndescription: the second decision, briefly\n---\n# Second\n\nBody.\n",
    );
    write(
        dir.path(),
        "docs/index.md",
        "# Index\n\n<!-- aigarden:cog index docs/0*.md -->\n<!-- aigarden:end -->\n",
    );
    assert_cmd_snapshot!(aigarden(dir.path()).args(["cog", "--write"]));
    insta::assert_snapshot!(
        "cog_index_contents",
        fs::read_to_string(dir.path().join("docs/index.md")).unwrap()
    );
}

#[test]
fn cog_fresh_surfaces_in_a_check_run() {
    let dir = tempfile::tempdir().unwrap();
    // A stale cog block is reported by `aigarden check` via the registry.
    write(
        dir.path(),
        "doc.md",
        "# Doc\n\n<!-- aigarden:cog sh \"echo hello\" -->\nstale\n<!-- aigarden:end -->\n",
    );
    assert_cmd_snapshot!(aigarden(dir.path()).arg("check"));
}

#[test]
fn cog_runs_only_where_cog_fresh_is_enabled() {
    let dir = tempfile::tempdir().unwrap();
    // A templates tree holds copyable placeholder cogs whose generators only work in
    // the repo they get copied into, so the config turns `cog-fresh` off there. The
    // cog subcommands must honor that exactly as `check` does: the template's
    // failing generator never runs (no exit 2), and only the real doc is gated.
    write(
        dir.path(),
        "aigarden.toml",
        "[per-file-ignores]\n\"templates/**\" = [\"cog-fresh\"]\n",
    );
    let template = "<!-- aigarden:cog sh \"exit 3\" -->\nplaceholder\n<!-- aigarden:end -->\n";
    write(dir.path(), "templates/AGENTS.md", template);
    write(
        dir.path(),
        "doc.md",
        "# Doc\n\n<!-- aigarden:cog sh \"echo hello\" -->\nstale\n<!-- aigarden:end -->\n",
    );
    assert_cmd_snapshot!(aigarden(dir.path()).args(["cog", "--check"]));
    // --write regenerates the real doc and leaves the exempt template byte-identical.
    assert_cmd_snapshot!(aigarden(dir.path()).args(["cog", "--write"]));
    assert_eq!(
        fs::read_to_string(dir.path().join("templates/AGENTS.md")).unwrap(),
        template
    );
}

#[test]
fn cog_with_cog_fresh_ignored_repo_wide_is_a_tool_error() {
    let dir = tempfile::tempdir().unwrap();
    // `ignore = ["cog-fresh"]` leaves the cog subcommands no file to act on. An
    // empty pass would read as "all fresh", so it is a loud exit 2 instead.
    write(dir.path(), "aigarden.toml", "ignore = [\"cog-fresh\"]\n");
    write(
        dir.path(),
        "doc.md",
        "<!-- aigarden:cog sh \"echo hello\" -->\nstale\n<!-- aigarden:end -->\n",
    );
    assert_cmd_snapshot!(aigarden(dir.path()).args(["cog", "--check"]));
}
