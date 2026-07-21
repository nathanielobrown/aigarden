# ailint design

**One-liner:** a Rust CLI that lints and maintains repositories for AI-agent + human collaboration — link/reference integrity, context-size budgets, and generated-content freshness — extracted from the doc-hygiene gates a large AI-authored codebase grows by hand.

The premise: when agents write most of the code and docs, conventions that a human reviewer used to hold in their head have to be mechanized, because nobody is reading every diff. A gate is better documentation than a sentence. `ailint` turns each convention into a rule that runs in one pass and reports everything.

## Architecture

```mermaid
flowchart TD
    cli[cli<br>clap subcommands + --output-format] --> config_load[config_load<br>ailint.toml strong defaults + shared excludes]
    config_load --> file_walker[file_walker<br>gitignore-aware walk]
    file_walker --> reference_extraction[reference_extraction<br>links, @-imports, bare paths, anchors]

    cli --> check_cmd[check_cmd]
    cli --> cog_cmd[cog_cmd<br>--check / --write]
    cli --> mv_cmd[mv_cmd<br>move + rewrite references]

    check_cmd --> rule_engine[rule_engine<br>run every layer, report all]
    reference_extraction --> rule_engine
    rule_engine --> own_rules[own_rules<br>file-length, link layers, cog-fresh]
    rule_engine --> rumdl_lib[rumdl_lib<br>MD051 anchors + MD057 relative links]

    cog_cmd --> cog_engine[cog_engine<br>file-tree, first-sentences, index, embedded shell]
    mv_cmd --> reference_extraction

    own_rules --> diagnostics[diagnostics<br>accumulate findings]
    rumdl_lib --> diagnostics
    cog_engine --> diagnostics
    mv_cmd --> diagnostics
    diagnostics --> output_layer[output_layer<br>human / json / github]
```

The load-bearing shape: **one reference-extraction core**, shared by `check` (which lints references) and `mv` (which rewrites them). In the tool this design is extracted from, those two lived as independently-drifting regex copies kept in sync by hand — the single core removes that class of bug by construction.

## v1 rule catalog

Rules are kebab-case, no numeric codes. Every rule is on by default and individually toggleable.

**Reference integrity** — the six layers of the link gate, run together and all reported:

- `link-target` — a relative markdown link points at a file that exists on disk (via `rumdl_lib` MD057); extensionless wiki-style links try `.md`
- `anchor-resolves` — a `#fragment`, in-file or `other.md#section`, resolves to an actual heading, honoring GitHub anchor-slug rules (via `rumdl_lib` MD051)
- `import-target` — an `@path` import in an always-loaded file (`CLAUDE.md`, `AGENTS.md`, `SKILL.md`) resolves on disk. These fail *silently* at runtime, so nothing else catches a broken one
- `bare-path` — a backticked, file-shaped path in markdown prose (interior slash, real-looking extension) exists relative to the file or repo root; git-ignored candidates are skipped as environment artifacts
- `link-case` — a link target's case matches the committed path exactly. macOS is case-insensitive, so a wrong-cased link passes locally and 404s on case-sensitive CI
- `code-doc-ref` — a doc path (`docs/…`, `issues/…`) cited inside a *non-markdown* source file exists. Root-relative only — nothing establishes a code file's doc directory

**Size budgets:**

- `file-length` — a file stays under its budget. The metric is chosen per file category, because "length" means different things: code files budget **lines** (human readability), while always-loaded guidance files and doc prose budget **chars/tokens** (context cost, ~4 chars/token). Config declares `(glob, metric, max)` groups. Line counting is true line count, not a newline count — a file missing its trailing newline is not silently one line short

**Markdown style:**

- `markdown-style` — reflow and style rules from `rumdl_lib`, surfaced under ailint's own config and diagnostics. Repos drop `.rumdl.toml`: one tool, one config

**Generated freshness:**

- `cog-fresh` — every generated cog block matches what its generator would produce now (see [Cogs](#cogs))

Mycelia-specific gates (diagram-tree axis tags, `§N` design-doc refs, tracker status headers) are deliberately **not** in v1 — see [roadmap.md](roadmap.md).

## Config model

`ailint.toml` at the repo root, ruff-style: strong defaults, an empty file mostly works, every rule toggleable, per-rule tables for thresholds and excludes. The walker already honors `.gitignore`; config `exclude` adds tool-level exclusions **defined once** and shared by every rule and by `mv` — the single home for what used to be a fixtures-exemption reimplemented in three separate tools.

```toml
# ailint.toml — everything on by default; override only what you need.

[exclude]
paths = ["**/fixtures/**", ".claude/worktrees/**"]  # skipped by every rule and by `mv`

# file-length: one budget group per file category, each with its own metric.
[[file-length.budget]]
glob = "**/*.rs"
metric = "lines"
max = 700
[[file-length.budget]]
glob = "{CLAUDE,AGENTS}.md"
metric = "tokens"   # ~4 chars/token
max = 4000

[link-case]
enabled = true      # every rule can be switched off the same way

[markdown-style]
reflow = true       # rumdl rules surface under ailint keys
```

## Cogs

A cog is a generated block whose body is recomputed on every run. Markers are HTML comments, so they vanish in rendered markdown:

```
<!-- ailint:cog file-tree src -->
...generated body, regenerated on every run...
<!-- ailint:end -->
```

The twist over a plain cog clone: a marker names **either** a built-in generator **or** an arbitrary shell command embedded in the marker itself (`<!-- ailint:cog sh "…" -->`). Built-ins are a deliberately non-Turing-complete template language; the shell escape hatch covers everything else without teaching the tool a scripting language.

Built-in trio for v1:

- `file-tree` — render a directory tree from a curated spec
- `first-sentences` — first sentence of each `- **Term** — gloss` bullet (the short-glossary pattern)
- `index` — a glob plus a per-entry gloss (ADR index, tracker index, …)

`ailint cog --check` gates freshness (prints a diff of any stale block, exits non-zero, never writes); `ailint cog --write` regenerates in place. The two are separate code paths on purpose — a check that regenerated-then-compared against itself would always pass. There is no default mode: the caller chooses.

## mv

`ailint mv <src> <dst>` stages a `git mv`, then rewrites **every reference form the link rules audit** — markdown links, backticked bare paths, and `@`-imports across markdown, plus root-relative doc-path tokens across non-markdown source. The moved file's own outbound relative links are re-anchored from its new directory. It uses the same reference-extraction core and the same exclusions as `check`, then re-runs the checks to confirm it left the repo clean.

## Output formats

`--output-format` selects the renderer, defaulting to `human`:

- `human` — annotated source snippets (rustc-style, via `annotate-snippets`)
- `json` — structured findings for agents and tooling
- `github` — GitHub Actions workflow-command annotations

Across all formats: the **green path is silent** (one pass line), the **red path is loud** (every finding). ailint owns this UX natively rather than wrapping each check in an output shim.

## Bugs fixed by design

Each of these was a known defect or drift risk in the hand-grown tooling this extracts from:

- **Report everything** — every rule runs and every finding surfaces in one pass; no stop-at-first-failing-layer that hides the next class of failure until you re-run
- **One reference-extraction core** — `check` and `mv` share it, so their link grammars cannot drift apart
- **Exclusions defined once** — the fixtures/worktree exemption lives in config, not reimplemented per rule
- **True line count** — `file-length` counts actual lines, not trailing-newline-sensitive `\n` count
- **Per-cog error boundary** — one failing generator reports itself; it does not crash the whole run
