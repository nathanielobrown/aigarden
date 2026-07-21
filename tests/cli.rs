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
fn code_doc_ref_flags_a_missing_doc_path_in_source() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "src/main.rs",
        "// rationale in docs/missing.md\nfn main() {}\n",
    );
    assert_cmd_snapshot!(ailint(dir.path()).arg("check"));
}
