# Contributor Guide for AI Agents

`aigarden` is a Rust CLI that enforces link integrity, context-size budgets, and freshness for generated content in mixed AI-human repositories. See `docs/design.md` for architecture and rules, and `docs/roadmap.md` for out-of-scope features.

This is an early pre-1.0 project with a single user: make breaking changes cleanly and skip compatibility shims.

## Toolchain and Commands

The toolchain is pinned in `rust-toolchain.toml`. Run workflows using `mise`:

- `mise run check`: The required gate. Runs `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, and `cargo test`.
- `mise run build` / `format` / `lint` / `test`: Run individual pipeline steps.
- `cargo insta test --review`: Run snapshot tests and review modifications interactively.
- `cargo insta accept`: Accept reviewed snapshot diffs.

## Engineering Rules

- **Write failing tests first.** Snapshot tests with `insta` and `insta-cmd` form the backbone. Capture rule diagnostics and full CLI execution output as snapshots.
- **Never hand-edit snapshot files.** Regenerate them via `insta` and inspect the diff.
- **Prefer `#[expect(...)]` over `#[allow(...)]`.** Unneeded suppressions will warn automatically.
- **Fail fast.** Validate configurations at startup. Never squash errors into silent skips or empty passes; zero matched files is a bug.
- **Route output through the diagnostics layer.** Do not use `print_stdout`, `print_stderr`, or `dbg!`, which trigger Clippy warnings. Return errors up the stack.

## Documentation Standards

Keep docstrings between 1 and 3 lines, focused on callers of primary interfaces. Explain the *why* directly at the relevant line. Add a one-line purpose note for each dependency and non-default configuration setting. Comment test cases liberally.
