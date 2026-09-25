//! Cog blocks meet `markdown-style`: the generator owns every byte between the
//! markers, so `check --fix` and `cog --write` must converge in either order
//! instead of undoing each other. See the "Cogs and markdown style" section of
//! `docs/design.md` for the invariant these tests pin down.

use std::fs;
use std::path::Path;

use insta_cmd::assert_cmd_snapshot;

mod common;
use common::{aigarden, write};

/// Run `aigarden <args>` in `dir` and assert it exits 0, so a convergence step
/// that leaves findings behind fails with the tool's own report.
fn succeeds(dir: &Path, args: &[&str]) {
    let out = aigarden(dir).args(args).output().unwrap();
    assert!(
        out.status.success(),
        "`aigarden {}` failed:\n{}{}",
        args.join(" "),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
}

/// A repo whose cog blocks exercise every way generated output can meet the
/// curated style set. Bodies start empty, so the first `cog --write` has work.
/// Generator outputs are synthetic, modeled on hyphae's `gen_layout` block.
fn seed_repo(dir: &Path) {
    // never-wrap puts MD013 in the style set, so reflow meets generated prose.
    write(
        dir,
        "aigarden.toml",
        "[markdown-style]\nreflow = \"never-wrap\"\n",
    );
    // Output that opens and closes with a fence: the MD031 loop from the bug report.
    write(
        dir,
        "gen/layout.txt",
        "```text\nsrc/hyphae/   Analyze AI coding agents from their telemetry\n```\n",
    );
    // The same fence padded with blank lines: the workaround downstream repos ship.
    write(
        dir,
        "gen/padded.txt",
        "\n```text\nsrc/save/   Upload a file\n```\n\n",
    );
    // Two long one-line paragraphs: prose never-wrap must leave alone, and must
    // never join onto the end marker below it.
    write(
        dir,
        "gen/prose.txt",
        "The first generated paragraph runs well past eighty columns so that a wrapping reflow would want to touch it.\n\nThe second generated paragraph also runs long, and it sits directly above the end marker line.\n",
    );
    write(
        dir,
        "doc.md",
        "# Doc\n\nAuthored prose with trailing spaces   \n\n\
         <!-- aigarden:cog sh \"cat gen/layout.txt\" -->\n<!-- aigarden:end -->\n\n\
         <!-- aigarden:cog sh \"cat gen/padded.txt\" -->\n<!-- aigarden:end -->\n\n\
         <!-- aigarden:cog sh \"cat gen/prose.txt\" -->\n<!-- aigarden:end -->\n\n\
         Authored closing line.\n",
    );
}

/// Seed a repo, run `order`, then prove the invariant: both gates pass, and a
/// second round of both commands changes nothing. Returns the converged doc.
fn converge(order: [&[&str]; 2]) -> String {
    let dir = tempfile::tempdir().unwrap();
    seed_repo(dir.path());
    for step in order {
        // `check --fix` exits 1 while findings remain, so only its write matters here.
        aigarden(dir.path()).args(step).output().unwrap();
    }
    succeeds(dir.path(), &["check"]);
    succeeds(dir.path(), &["cog", "--check"]);
    let converged = fs::read_to_string(dir.path().join("doc.md")).unwrap();
    // A second round of both commands is a no-op: nothing left to undo.
    succeeds(dir.path(), &["cog", "--write"]);
    succeeds(dir.path(), &["check", "--fix"]);
    assert_eq!(
        fs::read_to_string(dir.path().join("doc.md")).unwrap(),
        converged,
        "a second round of cog --write + check --fix changed the file"
    );
    converged
}

#[test]
fn cog_write_and_check_fix_converge_in_either_order() {
    let cog_first = converge([&["cog", "--write"], &["check", "--fix"]]);
    let fix_first = converge([&["check", "--fix"], &["cog", "--write"]]);
    // Order must not matter: both paths land on the same bytes.
    assert_eq!(cog_first, fix_first);
    // The fence hugs its markers unpadded, the padded block keeps its blank lines,
    // the prose stays on its own lines, and only the authored trailing spaces went.
    insta::assert_snapshot!(cog_first);
}

#[test]
fn a_fence_touching_a_cog_marker_is_not_an_md031_finding() {
    let dir = tempfile::tempdir().unwrap();
    // A fresh block whose body is a bare fence, flush against both markers. The
    // markers are HTML comments that end their own block, so the fence renders
    // and needs no padding. The authored fence below hugs prose, so MD031 still
    // fires there — marker adjacency is the only exemption.
    write(dir.path(), "gen/fence.txt", "```\ncode\n```\n");
    write(
        dir.path(),
        "doc.md",
        "# Doc\n\n<!-- aigarden:cog sh \"cat gen/fence.txt\" -->\n```\ncode\n```\n<!-- aigarden:end -->\n\nProse\n```\nauthored\n```\n",
    );
    assert_cmd_snapshot!(aigarden(dir.path()).arg("check"));
}

#[test]
fn style_findings_inside_a_cog_block_are_reported_not_fixed() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "aigarden.toml",
        "[markdown-style]\nreflow = \"never-wrap\"\n",
    );
    // The generator emits trailing spaces (MD009), a blank-line run (MD012), a
    // hard-wrapped paragraph with a hard tab (MD013, MD010), and a fence hugging
    // its own prose (MD031). Each is a real defect, so each is reported — but the
    // fix belongs in the generator, not in the file.
    let messy = "trailing   \n\n\n\nwrapped\tacross\ntwo lines\n```\nfenced\n```\n";
    write(dir.path(), "gen/messy.txt", messy);
    let doc = format!(
        "# Doc\n\n<!-- aigarden:cog sh \"cat gen/messy.txt\" -->\n{messy}<!-- aigarden:end -->\n"
    );
    write(dir.path(), "doc.md", &doc);
    assert_cmd_snapshot!(aigarden(dir.path()).args(["check", "--fix"]));
    // `--fix` wrote nothing: every byte between the markers is the generator's.
    assert_eq!(fs::read_to_string(dir.path().join("doc.md")).unwrap(), doc);
}

#[test]
fn fix_refuses_a_file_whose_cog_blocks_do_not_parse() {
    let dir = tempfile::tempdir().unwrap();
    // No end marker, so nothing says where generated text stops. Guessing could
    // rewrite generator output, so `--fix` stops with a tool error (exit 2) and
    // writes nothing — not even the authored trailing spaces.
    let doc = "# Doc\n\ntrailing   \n\n<!-- aigarden:cog sh \"echo hi\" -->\nhi\n";
    write(dir.path(), "doc.md", doc);
    assert_cmd_snapshot!(aigarden(dir.path()).args(["check", "--fix"]));
    assert_eq!(fs::read_to_string(dir.path().join("doc.md")).unwrap(), doc);
}
