# Mycelia parity — shadow-run report

The validation gate for adopting ailint in mycelia in place of its
hand-rolled `tools/` lint gates. It shadow-runs ailint against mycelia's whole working tree,
diffs findings against mycelia's own gates, classifies every discrepancy, and records what was
fixed, what is a deferred gap, and what is a deliberate non-goal. Read it to decide the cutover.

## Verdict

**At parity on the shared surface, with every named gap now closed.** After the in-scope fixes
below, ailint's residual over mycelia was **193 findings from one unbuilt feature** (the
high-severity frozen-history exemption) — which has since **shipped** as the generic `status-header`
rule, bringing the residual to zero once configured. Four gaps this run named — the `code-doc-ref`
fixture exemption, the `descriptive-anchor` rule, gitignored-candidate skipping, and the
frozen-history exemption — have all shipped (see
[Gaps closed since the run](#gaps-closed-since-the-run)). Zero misses and zero grammar-level false
positives remain.

| Category | Before fixes | After fixes | Where it went |
|---|---:|---:|---|
| 1 — ailint miss (mycelia finds, ailint doesn't) | 0 observable | 0 observable | mycelia's suite is green on this tree, so misses can't surface here (see Method) |
| 2 — ailint extra / **win** (genuine, mycelia can't see) | 1 | 1 | `backend/scripts/…` length scope-gap — kept |
| 3 — ailint false positive | 515 | 0 | ~101 grammar FPs fixed in code; 192 `code-doc-ref` + 6 `reports` suppressed by tuned config |
| 4 — deferred feature (not a non-goal, just unbuilt) | — | 193 | 193 frozen-history — has since **shipped** as `status-header` (see [Gaps closed](#gaps-closed-since-the-run)) |

## Method

**Toolchains, both on the same tree:**

- **mycelia** — commit `456a005` (branch `restore-pr-430`), clean tree. Gates run via its own
  `mise run` tasks (read-only; nothing modified in that repo): `lint-file-lengths` /
  `fe-lint-file-lengths`, `lint-links` (lychee `0.24.2` + five in-repo layers), `lint-diagrams`,
  `cogs-check`. **All green — 0 findings.**
- **ailint** — commit `1a1e2d0` (this repo, after the fixes below), `cargo build --release`,
  rumdl_lib `0.2.39`. Run as `ailint check --config <tuned> --output-format json` from the mycelia
  root over the same 1274 files.

**The tuned config** translates mycelia's gate wiring into ailint keys (excludes union +
dir-scoped length budgets with `use-defaults = false` + `markdown-style` off, since mycelia's rumdl
set — MD013 reflow + MD031 — differs from ailint's MD009/010/012/047). Its full text is in the
handoff; it lives in `/tmp`, never committed to mycelia.

**Limitation — the green-tree blind spot.** mycelia's suite passing means category 1 (ailint
*misses* something mycelia catches) is structurally unobservable on this tree: there is nothing for
either tool to find on the shared rules. Parity here is proven in the *extras* direction (every
ailint finding mycelia doesn't emit is explained) and by construction (ailint's reference core
re-implements the same six link layers). A tree with live violations would be needed to close the
miss direction empirically; the fixture-level round-trip tests in each rule module are the current
stand-in.

## Fixes made (categories 1 & 3, TDD, committed)

- **`0877015` — bare-path grammar (category 3, ~101 FPs).** mycelia's `check_bare_paths` rejects a
  backtick span before treating it as a path if it holds shell/glob metacharacters, a `NNNN`
  placeholder, `..`, or a `.git/` prefix, and skips spans that are the *label* of a markdown link.
  ailint's `looks_like_path` now applies the same guards (`BAD_SPAN_CHARS`, `NNNN`, `.git/`), and
  `extract_markdown` tracks link/image depth to ignore backticked code inside a link label. Two new
  tests pin both.
- **`1a1e2d0` — length over-coverage + fixtures default (categories 3 & D3).** `use-defaults =
  false` lets the tuned config run *only* mycelia's dir-scoped budgets, dropping 2 FPs the generic
  `**/*.py` default produced outside mycelia's gated dirs. `**/fixtures/**` became a built-in
  exclude (see D3). Four new config tests.

The remaining category-3 items — 192 `code-doc-ref` and 6 `reports` FPs — are suppressed by the
tuned config, **not** a code fix. At run time a faithful mapping needed *per-rule* scoping ailint
lacked (a global exclude would also drop those files from `file-length`); that scoping has since
shipped as [`[[overrides]]`](design.md#per-path-overrides), so the tuned config now disables
`code-doc-ref` for the fixture-bearing files via an override rather than a workaround (see
[Gaps closed since the run](#gaps-closed-since-the-run)).

## Wins (category 2)

- **Length coverage beyond mycelia's dir list.** mycelia gates `backend/mycelia backend/tests
  tools` for Python; a long script under `backend/scripts/` therefore escapes its length gate but
  ailint's generic default catches it. Mild — mycelia scoped this out on purpose — but a real
  scope gap surfaced by the run, and the kind of drift a generic default is meant to stop.
- **By construction, not counted above:** ailint reports *all* findings in one pass (mycelia's
  layered bash gates short-circuit), shares one reference-extraction core across the six link rules
  and `mv` (mycelia re-implements extraction per gate), and fails loudly on a malformed cog block.
  These are design parity dividends, not tree findings.

## Remaining gaps, by severity

**None** — every gap this run named has shipped (see below). ailint's residual over mycelia is now
zero once the `[status-header]` contract is configured.

### Gaps closed since the run

Four gaps this run named have shipped:

- **Frozen-history / status-header exemption (was High, 193 findings)** — shipped as the generic
  [`status-header`](design.md) rule: a `**Status:** <value>` contract with a live/terminal vocabulary
  where a terminal (frozen) doc's path citations are exempt from `bare-path`/`link-case`/
  `descriptive-anchor`, keyed off status not path. Closes the run's entire 193-finding residual (40
  frozen docs), the single biggest parity gap. Configured via `[status-header]` in `ailint.toml`
- **Gitignored-candidate skipping (was Low, 1 finding)** — `bare-path` and `code-doc-ref` now skip a
  *candidate target* that resolves to a gitignored path (an environment artifact present locally but
  absent on a fresh checkout), via the `ignore` crate the walker already uses. Closes the run's last
  residual false positive

- **Per-rule scoping (was Med, 192 findings masked by a workaround)** — shipped as
  [`[[overrides]]`](design.md#per-path-overrides): a glob-scoped, ruff-style mechanism where an
  override replaces a rule's whole table for matching files, base < overrides, later wins. Disabling
  `code-doc-ref` for the fixture-bearing files is now a first-class config, not a global-exclude
  workaround. ailint dogfoods it — see this repo's `ailint.toml`
- **`descriptive-anchor` (was Med, guards 1189 mycelia links)** — shipped as a config-driven rule
  (`[descriptive-anchor] patterns = [...]`), inert until patterns are declared. See D1 below

**Deliberate non-goals** (category 4, will never reach parity, by design): diagram-tree integrity,
numbered-section (`§N`) citations, and symlink "farms" are domain-shaped one-offs listed under
[Generalized versions of single-repo gates](roadmap.md#generalized-versions-of-single-repo-gates);
wrapping ruff/pyrefly/prettier is [out of scope](roadmap.md#explicitly-out-of-scope). ailint's cog
marker grammar (`ailint:cog`/`ailint:end`) is a clean break from mycelia's `[[[cogs]]]`, so cog
*content* can reach parity but the *marker syntax* intentionally won't.

## Design verdicts (Task D)

- **D1 — is the `descriptive-anchor` rule load-bearing? Yes — now shipped.** It guards a
  **1189-link** surface in mycelia (stable-ID citations like `ADR-NNNN`, `T\d+`, `P\d+` that must
  carry descriptive text, not a bare ID). It emits **0 findings** today, so it was not urgent, but
  it *is* the one deferred link layer mycelia relies on, and a cutover that dropped it would silently
  lose a real gate. Shipped config-driven (the ID shapes are `[descriptive-anchor] patterns`
  regexes), not as a hardcoded rule, and inert until configured.
- **D2 — are the default token caps sane? Yes, keep them.** Guidance cap **4000** is comfortable:
  the largest real guidance file (`CLAUDE.md`) is **2269** tokens, and **0** of mycelia's guidance
  files exceed it. General-markdown cap **8000** fires on **8 of 402** `.md` files — all genuinely
  large design docs (two plans at ~18k/17k tokens, `CONTEXT.md` at ~10.9k) — which is a gate doing
  its job, not noise. mycelia would layer a per-glob override for `plans/`; no change to the
  defaults.
- **D3 — do empty default excludes hold up, or is `**/fixtures/**` needed? Needed — changed the
  default (committed `1a1e2d0`).** A tracked 754-line workflow fixture exceeds the 700-line default,
  and `.gitignore` covers build output, not tracked fixtures, so nothing else skips it. Every
  hygiene tool in mycelia already excludes fixtures; ailint now does too, out of the box, unioned
  with (never replaced by) user excludes.
