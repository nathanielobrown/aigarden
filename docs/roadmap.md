# ailint roadmap

Future ideas, deliberately out of v1. v1 is the general core: reference integrity, size budgets, markdown style, and cogs (see [design.md](design.md)). Everything here is a candidate for later, roughly in order of how general it is.

## External link liveness

An `--online` mode for the link rules: resolve external URLs over the network, not just filesystem-resolvable references. This needs a network-checker's accept-list for anti-bot responses (treat 403/503 from sites that block non-browser clients as alive, not rot) and a run cadence of its own — external links rot on the doc's schedule, not the repo's, so this belongs on-demand and periodically, never in the fast inner-loop `check`. Keep the async network dependency tree out of the offline core.

## Generalized versions of single-repo gates

Gates that exist today as one-off, hardcoded checks — worth porting only once generalized behind config:

- **Numbered-section citations** — a `§N` (or configurable pattern) in source must match a numbered heading in a specific design doc. Generalize to a config triple `(citation_pattern, doc_path, source_glob)` rather than hardcoding one doc and one source tree
- **Diagram-tree integrity** — a directory of diagrams as a zoom hierarchy crossed with named axes: axis-tag filenames, no dangling drill-down links, no orphan sub-diagrams. Highly domain-shaped; port only if a second repo wants it
- **Status-header contracts** — issues/plans whose lifecycle state lives in a `**Status:**` header (never in the folder), with a vocabulary check and a "terminal-status files are frozen" exemption that some link rules honor. **The single biggest parity gap** (see [mycelia-parity.md](mycelia-parity.md)): a terminal-status doc may legitimately cite old, now-gone paths as historical record, so the bare-path/link-case/anchor rules must skip it. Without this, the mycelia shadow-run's entire residual — 193 bare-path findings across 40 frozen (`done`/`implemented`/`wontfix`) docs, zero live — is noise

## Token/char budgets inside code files

v1 budgets whole files. A finer rule: budget the **doc content within** a code file — a module or function docstring, or the running total of comment prose — in tokens, so an always-loaded source file can't bloat its guidance without tripping a gate. Extends the chars/tokens metric from file-level to span-level.

## Self-consistency gates

- **CI-vs-task drift** — assert that a CI workflow's per-gate step list matches the task runner's aggregate definition, so the two hand-kept parallel lists can't silently diverge. A natural thing for ailint to dogfood on its own repo
- **Config schema** — emit a JSON schema for `ailint.toml` (à la ruff/rumdl) for editor completion and validation

## More cog generators

Beyond the built-in trio, generators seen in practice: an ADR index that flattens links inside status lines, a layout tree that lifts each entry's first descriptive line from the target file, symlink "farms" materializing a live-items view. Add as demand appears; the embedded-shell escape hatch covers the long tail meanwhile.

The mycelia shadow-run (see [mycelia-parity.md](mycelia-parity.md)) grounds two of these:

- **`first-sentences` projection options** — the built-in reproduces mycelia's `CONTEXT_SHORT.md` structure and first-sentence cutting faithfully, but mycelia's generator also **compacts links** (an ADR link → bare `ADR-NNNN`, others → a backticked target), **drops** `_(future)_`/`_(extended)_`-marked terms, **enforces** per-line and total char budgets, and **suppresses** prose-only sections. These are opt-in flags a cutover would need
- **Curated layout tree** — `file-tree` dumps a whole real directory; mycelia's Layout is a *curated* subset with hand-authored annotations and docstring-lifting. A different generator (DSL-driven, as noted above), not a variant of `file-tree`

## Explicitly out of scope

Wrapping external linters/formatters/typecheckers (ruff, pyrefly, prettier), API-contract or test-fixture generation, and operator tooling (dev servers, metrics, release plumbing) are **not** ailint's job — it is repo hygiene for AI+human navigation, not a build system.
