# ailint 🌱

A Rust CLI that lints and maintains repositories built for **AI agents and humans to collaborate in**.

When agents write most of the code and docs, three things rot faster than a human reviewer can catch:

- **Reference integrity** — links, `@`-imports, and bare file paths in docs drift as files move; a broken `@import` fails silently at runtime, and a wrong-cased path passes on macOS but 404s on Linux CI
- **Context-size budgets** — files that agents load every session (like `CLAUDE.md`) have a token budget; blow it and every future session pays. Code files have line budgets for human readability
- **Generated-content freshness** — indexes, layout trees, and summaries computed from the repo go stale the moment their source changes

`ailint` mechanizes all three. It runs every check in one pass and reports everything (no stop-at-first-failure), shares one reference-extraction core between linting and the reference-rewriting `mv`, and defines every exclusion once in config.

## Install

```sh
cargo install --path .
```

Requires a recent stable Rust toolchain (pinned in `rust-toolchain.toml`).

## Quickstart

```sh
ailint check                 # run every lint layer over the current repo
ailint check --fix           # apply fixes where a rule supports them
ailint cog --check           # fail if any generated block is stale
ailint cog --write           # regenerate the stale blocks
ailint mv old.md new/dir/    # move a file and rewrite every reference to it
ailint rules                 # list the rules and their status
ailint explain bare-path     # print one rule's full contract
ailint check --output-format json   # machine-readable output for CI/agents
```

Configuration lives in `ailint.toml` at the repo root — strong defaults, every rule toggleable, per-glob thresholds. An empty file mostly just works. See `docs/design.md` for the config model and the full rule catalog.

## Status

Pre-1.0 and single-user: built to be published, but shaped around one person's repos first. Expect breaking changes to the config format and rule names. The CLI shell is in place; rules are landing incrementally.
