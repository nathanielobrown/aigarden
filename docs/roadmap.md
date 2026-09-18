# Roadmap

Post-v1 proposals ordered by generality. The v1 baseline provides reference integrity, size budgets, Markdown style, and cogs (see [design.md](design.md)).

## External link verification

Add an `--online` mode to validate external URLs alongside local filesystem targets.
- **Bot-blocking tolerances**: Treat HTTP 403 and 503 responses from scraper-hostile domains as active links rather than rot.
- **Execution profile**: External targets degrade independently of local commits. Run this check on a scheduled or manual trigger, keeping async network dependencies out of the local `check` path.

## Generalized repo checks

Convert hardcoded single-repo validations into configurable gates:

- **Numbered citations**: Generalize section references like `§N` using a `(citation_pattern, doc_path, source_glob)` config triple, replacing logic tied to specific files.
- **Diagram hierarchies**: Validate cross-axis zoom directories to detect orphan sub-diagrams and broken drill-down links. Retain this check until multiple projects require it.

The terminal-status frozen-docs check has already shipped as the generic `status-header` rule (see [design.md](design.md)). In mycelia shadow runs, it resolved all 193 bare-path findings across 40 frozen (`done`/`implemented`/`wontfix`) documents with zero remaining failures.

## In-code token and character limits

While v1 budgets whole files, this gate enforces token limits on documentation blocks inside code files, including function docstrings, module headers, and aggregate comment prose. This prevents always-loaded source context from silently bloating.

## Self-consistency rules

- **Workflow drift detection**: Compare CI pipeline definitions against task runner lists to prevent parallel execution definitions from silently desynchronizing.
- **Configuration schema**: Publish a JSON schema for `aigarden.toml` to power editor completions and schema validation.

## Additional cog generators

The built-in generators and embedded-shell fallback handle basic transformations. Further built-ins depend on user demand:

- **`first-sentences` extensions**: Grounded by mycelia parity tests (see [mycelia-parity.md](mycelia-parity.md)). Features needed for total parity include link compaction (`ADR-NNNN` conversions, target backticking), stripping `_(future)_` and `_(extended)_` markers, prose-only block suppression, and per-line character budgets.
- **Curated layout tree**: Generate an annotated structure from a DSL that lifts descriptive headers and docstrings from selected source files, rather than mirroring raw directory trees via `file-tree`.
- **Specialized layouts**: Format ADR indices with flattened status links and generate symlink structures for active documentation targets.

## Explicit exclusions

aigarden focuses strictly on repo navigation hygiene for AI and human maintainers. The following areas are out of scope:
- Wrapping linters, formatters, or typecheckers (`ruff`, `pyrefly`, `prettier`)
- Generating API contracts, mocks, or test fixtures
- Operator infrastructure, local dev servers, metrics, and release workflows
