# aigarden 🌱

A Rust CLI that lints and maintains repositories shared by AI agents and human developers.

Agent-driven repositories rot quickly in three distinct ways:

- **Reference drift**: Moved files break links, `@`-imports, and bare paths. Case mismatches pass on macOS but fail on Linux CI, while broken `@import` directives fail silently at runtime.
- **Budget overruns**: Bloated context files (such as `AGENTS.md`) waste token budgets on every agent run, while oversized code files degrade human readability.
- **Stale generated content**: Repo-derived summaries, index files, and layout trees drift immediately when source files change.

`aigarden` validates all three areas in a single non-halting pass, shares an extraction core across linting and path-rewriting commands, and applies exclusions from a unified configuration.

## Quickstart

```sh
aigarden check                 # run every lint layer over the repo
aigarden check --fix           # apply automated fixes
aigarden cog --check           # verify generated blocks match their sources
aigarden cog --write           # regenerate stale blocks
aigarden mv old.md new/dir/    # move a file and update all references to it
aigarden rules                 # list rules and their statuses
aigarden explain bare-path     # print a rule's contract
aigarden check --output-format json   # emit JSON for CI or agent pipelines
```

Configuration resides in `aigarden.toml` at the repository root, supporting rule toggles and per-glob thresholds. An empty file uses the default settings. See `docs/design.md` for rule catalogs and configuration details.

## Installation

Prebuilt binaries for `x86_64-unknown-linux-gnu` and `aarch64-apple-darwin` accompany each [release](https://github.com/nathanielobrown/aigarden/releases).

### mise

```sh
mise use github:nathanielobrown/aigarden@0.1.1
```

> **Note:** Pin to an exact release rather than `latest`. mise caches the initial resolution of `latest` permanently across upgrades. If installing immediately after a release, bypass the age quarantine by adding `minimum_release_age_excludes = ["github:nathanielobrown/aigarden"]` under `[settings]`.

### Direct Download

```sh
VERSION=0.1.1 TARGET=aarch64-apple-darwin   # or x86_64-unknown-linux-gnu
curl -fsSL "https://github.com/nathanielobrown/aigarden/releases/download/v$VERSION/aigarden-$VERSION-$TARGET.tar.gz" | tar -xz
```

### From Source

Requires the toolchain specified in `rust-toolchain.toml`:

```sh
cargo install --path .
```

## Releasing

Update `version` in `Cargo.toml` and merge to `main`. The `.github/workflows/release.yml` workflow triggers automatically to build and publish unreleased versions on `main`.

## Status

Pre-1.0 and single-user. Built for publication, but tailored initially to the author's workflow. Rule names and configuration schemas remain subject to breaking changes as rules land incrementally.
