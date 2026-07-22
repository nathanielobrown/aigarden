# Working guide for AI agents

`aigarden` is a Rust CLI that lints and maintains repositories for AI-agent + human collaboration: link/reference integrity, context-size budgets, and generated-content freshness. See `docs/design.md` for the architecture and rule catalog, `docs/roadmap.md` for what is deliberately out of v1.

This is an early, single-user, pre-1.0 project — make the clean breaking change, skip compat shims.

## Commands

Use `mise` (the toolchain is pinned in `rust-toolchain.toml`):

- `mise run check` — the one gate: `cargo fmt --check` + `cargo clippy --all-targets -- -D warnings` + `cargo test`. Green before you call a task done
- `mise run build` / `format` / `lint` / `test` — individual steps
- `cargo insta test --review` — run snapshot tests and review new/changed snapshots interactively
- `cargo insta accept` — accept pending snapshots after you've read the diff

## How we work here

- **TDD** — write the failing test first, then the code. Snapshot tests (`insta`, `insta-cmd`) are the backbone: a rule's diagnostics and a CLI run's whole output are captured as snapshots
- **Never hand-edit a snapshot file** — regenerate with insta and review the diff. A snapshot you typed by hand tests nothing
- **Prefer `#[expect(...)]` over `#[allow(...)]`** for suppressions — `expect` warns when the suppression becomes unnecessary, so dead exceptions self-report
- **Fail fast, crash loud** — validate config at startup, never squash an error into a silent skip or empty pass. Zero-files-found is a bug, not a no-op
- **All user-facing output goes through the diagnostics/output layer** — `print_stdout`/`print_stderr`/`dbg!` are clippy-warned; return errors up the stack instead

## Comments and docstrings

Brevity is a feature — comments are read far more than written. Write docstrings for a primary interface's callers; keep them 1–3 lines. Comment the *why* at the line it explains, not the *what*. Every non-default config setting and every dependency gets a one-line purpose note. Tests are the exception: comment them liberally to tell the case's story.
