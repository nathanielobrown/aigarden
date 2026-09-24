# Pull request fact sheet

The authoring agent (Claude) fills in this fact sheet from the final `git diff <base>...HEAD` and test output, never from the initial plan. `<base>` is `origin/main`, or the parent layer's branch for an upper stack layer.

The composer agent reads this file to draft the pull request description.

## Rules

- Use terse bullets. Do not polish or write prose.
- Omit any section or field that has nothing to say. Don't write "None" or leave placeholder text.
- Derive all technical claims, file paths, and snippets directly from the final diff and test execution.
- Save as `handoffs/pr-facts-<topic>.md` in the primary checkout. `handoffs/` is gitignored; never cite its paths in the PR.

---

### Tier and budget
- Tier: Light (~75 words) | Standard (~300 words) | Deep (~500 words). Pick the tier the change earns.

### Stack
- Stack goal: <1–2 sentences on overall stack goal if bottom PR; name bottom PR if upper layer; omit if not a stack>

### Intent
- What changed: <plain statement of changes from the diff>
- Why: <author rationale and motivation>

### Feedback wanted
- Review focus: <specific areas, questions, or architectural decisions requiring reviewer attention>

### Judgment points
- <path/to/file>: <risk, open decision, or known issue; order items by highest risk first>

### Design
- Design points: <architectural choices or subtleties not obvious from the diff alone>
- Plan deviations: <deviations from initial plan or design, if any>

### Visuals
- <Mermaid block for a new or changed flow, before/after CLI output from the changed snapshots, a before/after table for performance, or `save <file>` output: one line describing what it shows>

### Verification
- Checks executed: <commands run and output excerpts beyond standard green checks>
- Unverified areas: <code paths or scenarios not exercised>
- Test edits: <any modifications to tests, snapshots, or thresholds>

### Links
- Plan: <path or URL>
- Issue: <path or URL>
- Artifacts: <path or URL>

### Emphasis
- <free-form instructions to the composer, e.g. "focus on the API migration; keep the refactor brief">
