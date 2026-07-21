//! End-to-end CLI snapshots: run the real binary against throwaway repos and
//! capture stdout + stderr + exit code. These are the primary regression net —
//! they exercise config discovery, the walker, the engine, and every renderer.

use std::fs;
use std::path::Path;
use std::process::Command;

use insta_cmd::{assert_cmd_snapshot, get_cargo_bin};

/// A `Command` for the built `ailint` binary, rooted in `dir`.
fn ailint(dir: &Path) -> Command {
    let mut cmd = Command::new(get_cargo_bin("ailint"));
    cmd.current_dir(dir);
    cmd
}

/// Write `content` to `dir/name`, creating parent dirs.
fn write(dir: &Path, name: &str, content: &str) {
    let path = dir.join(name);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, content).unwrap();
}

#[test]
fn rules_lists_the_registered_rules() {
    let dir = tempfile::tempdir().unwrap();
    assert_cmd_snapshot!(ailint(dir.path()).arg("rules"));
}

#[test]
fn check_clean_repo_is_quiet_and_exits_zero() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "small.rs", "fn main() {}\n");
    assert_cmd_snapshot!(ailint(dir.path()).arg("check"));
}

#[test]
fn check_flags_a_file_over_the_default_line_budget() {
    let dir = tempfile::tempdir().unwrap();
    // 701 lines trips the built-in 700-line code budget.
    write(dir.path(), "big.rs", &"let _x = 0;\n".repeat(701));
    assert_cmd_snapshot!(ailint(dir.path()).arg("check"));
}

#[test]
fn check_json_output_has_the_stable_schema() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "big.rs", &"let _x = 0;\n".repeat(701));
    assert_cmd_snapshot!(ailint(dir.path()).args(["check", "--output-format", "json"]));
}

#[test]
fn check_github_output_emits_workflow_annotations() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "big.rs", &"let _x = 0;\n".repeat(701));
    assert_cmd_snapshot!(ailint(dir.path()).args(["check", "--output-format", "github"]));
}

#[test]
fn config_can_override_a_budget_with_a_custom_glob_and_token_metric() {
    let dir = tempfile::tempdir().unwrap();
    // A tokens budget on *.txt: 8 chars -> ceil(8/4) = 2 tokens > 1.
    write(
        dir.path(),
        "ailint.toml",
        "[[file-length.budget]]\nglob = \"**/*.txt\"\nmetric = \"tokens\"\nmax = 1\n",
    );
    write(dir.path(), "notes.txt", "abcdefgh");
    assert_cmd_snapshot!(ailint(dir.path()).arg("check"));
}

#[test]
fn unknown_config_key_fails_with_exit_two() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "ailint.toml", "[nonsense]\nfoo = 1\n");
    write(dir.path(), "small.rs", "fn main() {}\n");
    // The error names the config path, which is a random tempdir — redact it.
    insta::with_settings!({filters => vec![(r"\S*ailint\.toml", "[CONFIG]")]}, {
        assert_cmd_snapshot!(ailint(dir.path()).arg("check"));
    });
}

#[test]
fn no_files_found_fails_with_exit_two() {
    let dir = tempfile::tempdir().unwrap();
    assert_cmd_snapshot!(ailint(dir.path()).arg("check"));
}

#[test]
fn link_target_flags_a_broken_relative_link() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "doc.md",
        "See [the guide](guide.md) for details.\n",
    );
    assert_cmd_snapshot!(ailint(dir.path()).arg("check"));
}

#[test]
fn valid_links_including_extensionless_and_anchor_are_clean() {
    let dir = tempfile::tempdir().unwrap();
    // An existing target, an extensionless wiki-style link (tries guide.md), a
    // same-file anchor, a nested relative path, and an external URL — all fine.
    write(dir.path(), "guide.md", "# Guide\n");
    write(dir.path(), "sub/page.md", "Up to [top](../guide.md).\n");
    write(
        dir.path(),
        "doc.md",
        "[a](guide.md), [b](guide), [c](#top), [d](https://example.com).\n\n# Top\n",
    );
    assert_cmd_snapshot!(ailint(dir.path()).arg("check"));
}

#[test]
fn link_case_flags_a_wrong_cased_target_that_still_exists() {
    let dir = tempfile::tempdir().unwrap();
    // The file is `Guide.md`; the link says `guide.md`. On macOS it opens, on a
    // case-sensitive CI it 404s — link-case catches it while link-target stays quiet.
    write(dir.path(), "Guide.md", "# Guide\n");
    write(dir.path(), "doc.md", "See [the guide](guide.md).\n");
    assert_cmd_snapshot!(ailint(dir.path()).arg("check"));
}

#[test]
fn bare_path_flags_a_missing_backticked_path() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "doc.md",
        "Look at `src/missing.rs` in the tree.\n",
    );
    assert_cmd_snapshot!(ailint(dir.path()).arg("check"));
}

#[test]
fn import_target_flags_a_broken_at_import() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "CLAUDE.md", "@docs/missing.md\n");
    assert_cmd_snapshot!(ailint(dir.path()).arg("check"));
}

#[test]
fn guidance_files_in_hidden_dirs_are_walked() {
    let dir = tempfile::tempdir().unwrap();
    // SKILL.md lives under a hidden `.claude/` tree; its broken @-import must
    // still be caught (WS1's walker skipped hidden files by default).
    write(dir.path(), ".claude/skills/x/SKILL.md", "@../missing.md\n");
    assert_cmd_snapshot!(ailint(dir.path()).arg("check"));
}

#[test]
fn anchor_resolves_flags_missing_same_file_and_cross_file_fragments() {
    let dir = tempfile::tempdir().unwrap();
    // `guide.md` has one heading; the links reach for a missing cross-file anchor
    // and a missing same-file anchor. Both are MD051 findings under one rule.
    write(dir.path(), "guide.md", "# Guide\n\n## Real Heading\n");
    write(
        dir.path(),
        "doc.md",
        "See [x](guide.md#missing) and [y](#nope).\n\n# Top\n",
    );
    assert_cmd_snapshot!(ailint(dir.path()).arg("check"));
}

#[test]
fn markdown_style_flags_trailing_spaces_and_missing_final_newline() {
    let dir = tempfile::tempdir().unwrap();
    // Trailing whitespace (MD009) and no closing newline (MD047) — the fixable set.
    write(
        dir.path(),
        "s.md",
        "# Title\n\nA line with trailing   \nlast",
    );
    assert_cmd_snapshot!(ailint(dir.path()).arg("check"));
}

#[test]
fn fix_repairs_markdown_style_then_a_re_check_is_clean() {
    let dir = tempfile::tempdir().unwrap();
    // Two blank-line runs (MD012) plus a tab (MD010) plus a missing final newline.
    write(
        dir.path(),
        "s.md",
        "# Title\n\nPara one.\n\n\n\nPara two with a\ttab.",
    );
    // `--fix` rewrites the file, then reports the (now empty) residue.
    assert_cmd_snapshot!(ailint(dir.path()).args(["check", "--fix"]));
    // The file on disk is now canonical markdown.
    let fixed = fs::read_to_string(dir.path().join("s.md")).unwrap();
    insta::assert_snapshot!("fix_rewrites_file_contents", fixed);
    // A second plain check finds nothing — the fix was idempotent and complete.
    assert_cmd_snapshot!(ailint(dir.path()).arg("check"));
}

#[test]
fn code_doc_ref_flags_a_missing_doc_path_in_source() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "src/main.rs",
        "// rationale in docs/missing.md\nfn main() {}\n",
    );
    assert_cmd_snapshot!(ailint(dir.path()).arg("check"));
}

// ── cog ──────────────────────────────────────────────────────────────────────

#[test]
fn cog_requires_a_mode() {
    // Neither --check nor --write: clap rejects it (no default mode).
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "doc.md", "# Doc\n");
    assert_cmd_snapshot!(ailint(dir.path()).arg("cog"));
}

#[test]
fn cog_check_flags_a_stale_block() {
    let dir = tempfile::tempdir().unwrap();
    // The body says `stale` but the generator produces `hello`.
    write(
        dir.path(),
        "doc.md",
        "# Doc\n\n<!-- ailint:cog sh \"echo hello\" -->\nstale\n<!-- ailint:end -->\n",
    );
    assert_cmd_snapshot!(ailint(dir.path()).args(["cog", "--check"]));
}

#[test]
fn cog_write_regenerates_then_check_is_clean() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "doc.md",
        "# Doc\n\n<!-- ailint:cog sh \"echo hello\" -->\nstale\n<!-- ailint:end -->\n",
    );
    // --write splices the fresh body and reports the changed file.
    assert_cmd_snapshot!(ailint(dir.path()).args(["cog", "--write"]));
    let written = fs::read_to_string(dir.path().join("doc.md")).unwrap();
    insta::assert_snapshot!("cog_write_file_contents", written);
    // A --check right after --write is always clean (determinism).
    assert_cmd_snapshot!(ailint(dir.path()).args(["cog", "--check"]));
}

#[test]
fn cog_failing_generator_is_a_tool_error() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "doc.md",
        "<!-- ailint:cog sh \"exit 3\" -->\n<!-- ailint:end -->\n",
    );
    // A nonzero shell exit is exit 2 (tool error), not a finding.
    assert_cmd_snapshot!(ailint(dir.path()).args(["cog", "--check"]));
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
        "<!-- ailint:cog file-tree src -->\n<!-- ailint:end -->\n",
    );
    assert_cmd_snapshot!(ailint(dir.path()).args(["cog", "--write"]));
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
        "<!-- ailint:cog first-sentences CONTEXT.md -->\n<!-- ailint:end -->\n",
    );
    assert_cmd_snapshot!(ailint(dir.path()).args(["cog", "--write"]));
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
        "# Index\n\n<!-- ailint:cog index docs/0*.md -->\n<!-- ailint:end -->\n",
    );
    assert_cmd_snapshot!(ailint(dir.path()).args(["cog", "--write"]));
    insta::assert_snapshot!(
        "cog_index_contents",
        fs::read_to_string(dir.path().join("docs/index.md")).unwrap()
    );
}

#[test]
fn cog_fresh_surfaces_in_a_check_run() {
    let dir = tempfile::tempdir().unwrap();
    // A stale cog block is reported by `ailint check` via the registry.
    write(
        dir.path(),
        "doc.md",
        "# Doc\n\n<!-- ailint:cog sh \"echo hello\" -->\nstale\n<!-- ailint:end -->\n",
    );
    assert_cmd_snapshot!(ailint(dir.path()).arg("check"));
}

// ── mv ───────────────────────────────────────────────────────────────────────

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
