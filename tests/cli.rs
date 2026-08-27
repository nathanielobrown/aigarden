//! End-to-end CLI snapshots: run the real binary against throwaway repos and
//! capture stdout + stderr + exit code. These are the primary regression net —
//! they exercise config discovery, the walker, the engine, and every renderer.

use std::fs;

use insta_cmd::assert_cmd_snapshot;

mod common;
use common::{aigarden, write};

#[test]
fn rules_lists_the_registered_rules() {
    let dir = tempfile::tempdir().unwrap();
    assert_cmd_snapshot!(aigarden(dir.path()).arg("rules"));
}

#[test]
fn explain_prints_a_rules_full_contract() {
    // A fixable rule's contract: what it checks, its config keys with defaults, an
    // example finding, and what `--fix` does — all sourced from the rule itself.
    let dir = tempfile::tempdir().unwrap();
    assert_cmd_snapshot!(aigarden(dir.path()).args(["explain", "markdown-style"]));
}

#[test]
fn explain_covers_a_config_gated_rule() {
    // descriptive-anchor is on by default but inert until `patterns` is set — its
    // config-gated status and its `patterns` key must both surface.
    let dir = tempfile::tempdir().unwrap();
    assert_cmd_snapshot!(aigarden(dir.path()).args(["explain", "descriptive-anchor"]));
}

#[test]
fn explain_on_an_unknown_rule_is_a_clean_error_not_a_panic() {
    // A bad rule name is a tool error (exit 2) naming the known rules, never a panic.
    let dir = tempfile::tempdir().unwrap();
    assert_cmd_snapshot!(aigarden(dir.path()).args(["explain", "no-such-rule"]));
}

#[test]
fn check_clean_repo_is_quiet_and_exits_zero() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "small.rs", "fn main() {}\n");
    assert_cmd_snapshot!(aigarden(dir.path()).arg("check"));
}

#[test]
fn check_flags_a_file_over_the_default_line_budget() {
    let dir = tempfile::tempdir().unwrap();
    // 701 lines trips the built-in 700-line code budget.
    write(dir.path(), "big.rs", &"let _x = 0;\n".repeat(701));
    assert_cmd_snapshot!(aigarden(dir.path()).arg("check"));
}

#[test]
fn check_json_output_has_the_stable_schema() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "big.rs", &"let _x = 0;\n".repeat(701));
    assert_cmd_snapshot!(aigarden(dir.path()).args(["check", "--output-format", "json"]));
}

#[test]
fn check_github_output_emits_workflow_annotations() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "big.rs", &"let _x = 0;\n".repeat(701));
    assert_cmd_snapshot!(aigarden(dir.path()).args(["check", "--output-format", "github"]));
}

#[test]
fn config_can_override_a_budget_with_a_custom_glob_and_token_metric() {
    let dir = tempfile::tempdir().unwrap();
    // A tokens budget on *.txt: 8 chars -> ceil(8/4) = 2 tokens > 1. `extend-budgets`
    // adds it on top of the built-ins without re-listing them.
    write(
        dir.path(),
        "aigarden.toml",
        "[file-length.extend-budgets]\n\"**/*.txt\" = { tokens = 1 }\n",
    );
    write(dir.path(), "notes.txt", "abcdefgh");
    assert_cmd_snapshot!(aigarden(dir.path()).arg("check"));
}

#[test]
fn unknown_config_key_fails_with_exit_two() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "aigarden.toml", "[nonsense]\nfoo = 1\n");
    write(dir.path(), "small.rs", "fn main() {}\n");
    // The error names the config path, which is a random tempdir — redact it.
    insta::with_settings!({filters => vec![(r"\S*aigarden\.toml", "[CONFIG]")]}, {
        assert_cmd_snapshot!(aigarden(dir.path()).arg("check"));
    });
}

#[test]
fn bad_budget_glob_is_a_clean_config_error_not_a_panic() {
    let dir = tempfile::tempdir().unwrap();
    // A malformed `file-length` budget glob must be caught at config load as a
    // tool/config error (exit 2), not compiled lazily inside the rule where it
    // panics (exit 101). The message names the offending key and value.
    write(
        dir.path(),
        "aigarden.toml",
        "[file-length.extend-budgets]\n\"[unclosed\" = { lines = 1 }\n",
    );
    write(dir.path(), "a.rs", "fn main() {}\n");
    insta::with_settings!({filters => vec![(r"\S*aigarden\.toml", "[CONFIG]")]}, {
        assert_cmd_snapshot!(aigarden(dir.path()).arg("check"));
    });
}

#[test]
fn bad_descriptive_anchor_pattern_is_a_clean_config_error_not_a_panic() {
    let dir = tempfile::tempdir().unwrap();
    // A malformed `descriptive-anchor` regex is likewise a load-time config error
    // (exit 2), not a panic from the rule's lazy `Regex::new`.
    write(
        dir.path(),
        "aigarden.toml",
        "[descriptive-anchor]\npatterns = [\"ADR-(\\\\d+\"]\n",
    );
    write(dir.path(), "doc.md", "# Doc\n\nSee [ADR-1](x.md).\n");
    insta::with_settings!({filters => vec![(r"\S*aigarden\.toml", "[CONFIG]")]}, {
        assert_cmd_snapshot!(aigarden(dir.path()).arg("check"));
    });
}

#[test]
fn no_files_found_fails_with_exit_two() {
    let dir = tempfile::tempdir().unwrap();
    assert_cmd_snapshot!(aigarden(dir.path()).arg("check"));
}

#[test]
fn fix_help_text_names_the_fixable_rule_not_a_falsehood() {
    // `markdown-style` is fixable (and `--fix` works), so the old "no rule does
    // yet" help was false. The help must name the fixable rule and drop the lie.
    let dir = tempfile::tempdir().unwrap();
    let out = aigarden(dir.path())
        .args(["check", "--help"])
        .output()
        .unwrap();
    let help = String::from_utf8(out.stdout).unwrap();
    assert!(
        help.contains("markdown-style"),
        "--fix help should name the fixable rule; got:\n{help}"
    );
    assert!(
        !help.contains("no rule does yet"),
        "--fix help still claims no rule is fixable; got:\n{help}"
    );
}

#[test]
fn check_on_a_non_utf8_file_does_not_panic() {
    let dir = tempfile::tempdir().unwrap();
    // A markdown file with invalid UTF-8 bytes plus a broken link. The walker reads
    // it lossily (bytes → U+FFFD), and the link rule spans against that in-memory
    // copy. The human renderer must draw from the *same* in-memory content — never
    // re-read the file from disk, which decodes differently and panics the snippet
    // engine with an out-of-range span.
    fs::write(
        dir.path().join("bad.md"),
        [b"\xff\xfe\x00".as_slice(), b"See [x](nope.md).\n"].concat(),
    )
    .unwrap();
    assert_cmd_snapshot!(aigarden(dir.path()).arg("check"));
}

#[test]
fn link_target_flags_a_broken_relative_link() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "doc.md",
        "See [the guide](guide.md) for details.\n",
    );
    assert_cmd_snapshot!(aigarden(dir.path()).arg("check"));
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
    assert_cmd_snapshot!(aigarden(dir.path()).arg("check"));
}

#[test]
fn link_case_flags_a_wrong_cased_target_that_still_exists() {
    let dir = tempfile::tempdir().unwrap();
    // The file is `Guide.md`; the link says `guide.md`. On macOS it opens, on a
    // case-sensitive CI it 404s — link-case catches it while link-target stays quiet.
    write(dir.path(), "Guide.md", "# Guide\n");
    write(dir.path(), "doc.md", "See [the guide](guide.md).\n");
    assert_cmd_snapshot!(aigarden(dir.path()).arg("check"));
}

#[test]
fn bare_path_flags_a_missing_backticked_path() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "doc.md",
        "Look at `src/missing.rs` in the tree.\n",
    );
    assert_cmd_snapshot!(aigarden(dir.path()).arg("check"));
}

#[test]
fn bare_path_skips_a_gitignored_candidate() {
    let dir = tempfile::tempdir().unwrap();
    // A backticked path pointing at a generated, gitignored file: it exists locally
    // for the author but not on a fresh checkout, so flagging it would diverge local
    // from CI. A gitignored candidate is an environment artifact — not a finding.
    write(dir.path(), ".gitignore", "build/\n");
    write(
        dir.path(),
        "doc.md",
        "The bundle lands at `build/out.js` after a run.\n",
    );
    assert_cmd_snapshot!(aigarden(dir.path()).arg("check"));
}

#[test]
fn code_doc_ref_skips_a_gitignored_candidate() {
    let dir = tempfile::tempdir().unwrap();
    // Same environment-artifact rule for a doc path cited in source: a reference to a
    // gitignored, generated doc is skipped rather than flagged as missing.
    write(dir.path(), ".gitignore", "docs/generated/\n");
    write(
        dir.path(),
        "src/main.rs",
        "// see docs/generated/api.md for the emitted contract\nfn main() {}\n",
    );
    assert_cmd_snapshot!(aigarden(dir.path()).arg("check"));
}

#[test]
fn import_target_flags_a_broken_at_import() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "CLAUDE.md", "@docs/missing.md\n");
    assert_cmd_snapshot!(aigarden(dir.path()).arg("check"));
}

#[test]
fn guidance_files_in_hidden_dirs_are_walked() {
    let dir = tempfile::tempdir().unwrap();
    // SKILL.md lives under a hidden `.claude/` tree; its broken @-import must
    // still be caught (WS1's walker skipped hidden files by default).
    write(dir.path(), ".claude/skills/x/SKILL.md", "@../missing.md\n");
    assert_cmd_snapshot!(aigarden(dir.path()).arg("check"));
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
    assert_cmd_snapshot!(aigarden(dir.path()).arg("check"));
}

#[test]
fn markdown_style_flags_trailing_spaces_and_missing_final_newline() {
    let dir = tempfile::tempdir().unwrap();
    // Trailing whitespace (MD009), a fence with no blank line before it (MD031), and
    // no closing newline (MD047) — each reported under `markdown-style`, tagged with
    // the rumdl rule id it came from.
    write(
        dir.path(),
        "s.md",
        "# Title\n\nA line with trailing   \n```text\nfenced\n```\n\nlast",
    );
    assert_cmd_snapshot!(aigarden(dir.path()).arg("check"));
}

#[test]
fn fix_repairs_markdown_style_then_a_re_check_is_clean() {
    let dir = tempfile::tempdir().unwrap();
    // Two blank-line runs (MD012), a tab (MD010), a fence hugging the prose on both
    // sides (MD031), and a missing final newline (MD047) — the whole fixable set.
    write(
        dir.path(),
        "s.md",
        "# Title\n\nPara one.\n\n\n\nPara two with a\ttab.\n```text\nfenced\n```\nAfter the fence.",
    );
    // `--fix` rewrites the file, then reports the (now empty) residue.
    assert_cmd_snapshot!(aigarden(dir.path()).args(["check", "--fix"]));
    // The file on disk is now canonical markdown.
    let fixed = fs::read_to_string(dir.path().join("s.md")).unwrap();
    insta::assert_snapshot!("fix_rewrites_file_contents", fixed);
    // A second plain check finds nothing — the fix was idempotent and complete.
    assert_cmd_snapshot!(aigarden(dir.path()).arg("check"));
}

/// The never-wrap convention: one physical line per paragraph. `reflow =
/// "never-wrap"` must report every hard-wrapped paragraph and join it on `--fix`.
/// The messages must also read in aigarden's own terms — the column it hands rumdl
/// to spell "never wrap" is a sentinel, and a finding must never quote it.
#[test]
fn never_wrap_flags_a_hard_wrapped_paragraph_then_fix_joins_it() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "aigarden.toml",
        "[markdown-style]\nreflow = \"never-wrap\"\n",
    );
    // A paragraph and a list item, each split over short lines — under any column
    // limit, so only a normalize-style reflow can see them. rumdl phrases those two
    // findings differently, so both message shapes are covered here.
    let wrapped = "# Title\n\nA paragraph the author\nhard-wrapped across\nthree lines.\n\n- a list item the author\n  also hard-wrapped\n";
    write(dir.path(), "s.md", wrapped);
    // Check mode reports it...
    assert_cmd_snapshot!(aigarden(dir.path()).arg("check"));
    // ...and leaves the file alone: check never writes.
    assert_eq!(
        fs::read_to_string(dir.path().join("s.md")).unwrap(),
        wrapped
    );
    // Fix mode joins it, and reports the (now empty) residue.
    assert_cmd_snapshot!(aigarden(dir.path()).args(["check", "--fix"]));
    let fixed = fs::read_to_string(dir.path().join("s.md")).unwrap();
    insta::assert_snapshot!("never_wrap_joins_the_paragraph", fixed);
    // A second plain check is clean — check and fix agree on what never-wrap means.
    assert_cmd_snapshot!(aigarden(dir.path()).arg("check"));
}

/// Never-wrap is a *prose* convention: a code fence's line breaks are its content
/// and a table's are its structure, so both must survive a fix byte for byte.
#[test]
fn never_wrap_leaves_code_fences_and_tables_untouched() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "aigarden.toml",
        "[markdown-style]\nreflow = \"never-wrap\"\n",
    );
    // Every paragraph here is already one line, so the only multi-line blocks are
    // the fence and the table. A conforming file must be reported clean...
    let doc = "# Title\n\nAlready one line.\n\n```text\nfenced line one\nfenced line two\n```\n\n| col | other |\n| --- | --- |\n| a | b |\n| c | d |\n";
    write(dir.path(), "s.md", doc);
    assert_cmd_snapshot!(aigarden(dir.path()).arg("check"));
    // ...and survive `--fix` unchanged, byte for byte.
    assert_cmd_snapshot!(aigarden(dir.path()).args(["check", "--fix"]));
    assert_eq!(fs::read_to_string(dir.path().join("s.md")).unwrap(), doc);
}

/// The former `reflow = true` — rumdl's re-wrap-at-the-limit behavior — keeps
/// working under its own name, so never-wrap is an added mode, not a replacement.
#[test]
fn reflow_wrap_re_wraps_an_over_long_paragraph() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "aigarden.toml",
        "[markdown-style]\nreflow = \"wrap\"\n",
    );
    // One 100-column line: over rumdl's 80-column limit, so wrap mode breaks it.
    write(
        dir.path(),
        "s.md",
        "# Title\n\nword wordy wordier wordiest word wordy wordier wordiest word wordy wordier wordiest words.\n",
    );
    assert_cmd_snapshot!(aigarden(dir.path()).args(["check", "--fix"]));
    let fixed = fs::read_to_string(dir.path().join("s.md")).unwrap();
    insta::assert_snapshot!("reflow_wrap_wraps_at_the_limit", fixed);
}

/// `reflow` is a named mode, not a flag. The old boolean spelling must fail loudly
/// at load (exit 2) rather than being read as one of the modes.
#[test]
fn boolean_reflow_is_a_clean_config_error() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "aigarden.toml",
        "[markdown-style]\nreflow = true\n",
    );
    write(dir.path(), "s.md", "# Title\n\nA line.\n");
    insta::with_settings!({filters => vec![(r"\S*aigarden\.toml", "[CONFIG]")]}, {
        assert_cmd_snapshot!(aigarden(dir.path()).arg("check"));
    });
}

#[test]
fn per_file_ignores_scopes_a_single_rule_by_path() {
    let dir = tempfile::tempdir().unwrap();
    // grammar.rs both cites a fake doc path (a code-doc-ref finding) and is over a
    // tiny line budget. A per-file-ignores entry that disables *only* code-doc-ref
    // for that path must silence that rule while file-length still fires — a global
    // exclude would drop the file from every rule, so this proves per-path, per-rule
    // scoping.
    write(
        dir.path(),
        "aigarden.toml",
        "[file-length.extend-budgets]\n\"**/*.rs\" = { lines = 1 }\n\
         [per-file-ignores]\n\"grammar.rs\" = [\"code-doc-ref\"]\n",
    );
    write(
        dir.path(),
        "grammar.rs",
        "// example path docs/fake.md\nfn a() {}\n",
    );
    assert_cmd_snapshot!(aigarden(dir.path()).arg("check"));
}

#[test]
fn per_file_ignores_unions_rules_and_scopes_by_glob() {
    let dir = tempfile::tempdir().unwrap();
    // Two entries, ruff-style union semantics. `docs/**` turns off bare-path for the
    // whole tree; `docs/legacy.md` additionally turns off link-target. So legacy.md
    // (matched by both) has *both* silenced and is clean, while other.md keeps
    // link-target — its broken link is still flagged, its bad bare path is not.
    write(
        dir.path(),
        "aigarden.toml",
        "[per-file-ignores]\n\"docs/**\" = [\"bare-path\"]\n\"docs/legacy.md\" = [\"link-target\"]\n",
    );
    let body = "See [gone](missing.md) and `src/nope.rs` in the tree.\n";
    write(dir.path(), "docs/legacy.md", body);
    write(dir.path(), "docs/other.md", body);
    assert_cmd_snapshot!(aigarden(dir.path()).arg("check"));
}

#[test]
fn code_doc_ref_flags_a_missing_doc_path_in_source() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "src/main.rs",
        "// rationale in docs/missing.md\nfn main() {}\n",
    );
    assert_cmd_snapshot!(aigarden(dir.path()).arg("check"));
}

// ── cog ──────────────────────────────────────────────────────────────────────

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
