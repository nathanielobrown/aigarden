# aigarden 🌱

A Rust CLI that lints and maintains repositories built for **AI agents and humans to collaborate in**.

When agents write most of the code and docs, three things rot faster than a human reviewer can catch:

- **Reference integrity** — links, `@`-imports, and bare file paths in docs drift as files move; a broken `@import` fails silently at runtime, and a wrong-cased path passes on macOS but 404s on Linux CI
- **Context-size budgets** — files that agents load every session (like `CLAUDE.md`) have a token budget; blow it and every future session pays. Code files have line budgets for human readability
- **Generated-content freshness** — indexes, layout trees, and summaries computed from the repo go stale the moment their source changes

`aigarden` mechanizes all three. It runs every check in one pass and reports everything (no stop-at-first-failure), shares one reference-extraction core between linting and the reference-rewriting `mv`, and defines every exclusion once in config.

## Install

Every [release](https://github.com/nathanielobrown/aigarden/releases) ships prebuilt binaries for linux x86_64 and macOS arm64, so you don't need a Rust toolchain.

**With mise:**

```sh
mise use github:nathanielobrown/aigarden@0.1.1
```

Pin an exact version rather than `latest` — mise resolves a `latest` request once and then treats it as satisfied, so later releases never arrive (no `mise upgrade` or cache clear dislodges it). If you pin a version minutes after it's cut, mise's release-age quarantine will refuse it; add `minimum_release_age_excludes = ["github:nathanielobrown/aigarden"]` under `[settings]`.

**Direct download:**

```sh
VERSION=0.1.1 TARGET=aarch64-apple-darwin   # or x86_64-unknown-linux-gnu
curl -fsSL "https://github.com/nathanielobrown/aigarden/releases/download/v$VERSION/aigarden-$VERSION-$TARGET.tar.gz" | tar -xz
```

**From source**, which needs the toolchain pinned in `rust-toolchain.toml`:

```sh
cargo install --path .
```

## Releasing

Bump `version` in `Cargo.toml` and merge to main — `.github/workflows/release.yml` builds and publishes any version on main that has no release yet.

## Quickstart

```sh
aigarden check                 # run every lint layer over the current repo
aigarden check --fix           # apply fixes where a rule supports them
aigarden cog --check           # fail if any generated block is stale
aigarden cog --write           # regenerate the stale blocks
aigarden mv old.md new/dir/    # move a file and rewrite every reference to it
aigarden rules                 # list the rules and their status
aigarden explain bare-path     # print one rule's full contract
aigarden check --output-format json   # machine-readable output for CI/agents
```

Configuration lives in `aigarden.toml` at the repo root — strong defaults, every rule toggleable, per-glob thresholds. An empty file mostly just works. See `docs/design.md` for the config model and the full rule catalog.

## Status

Pre-1.0 and single-user: built to be published, but shaped around one person's repos first. Expect breaking changes to the config format and rule names. The CLI shell is in place; rules are landing incrementally.
