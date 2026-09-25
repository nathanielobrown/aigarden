# Pull request fact sheet

The fact sheet is a verbose draft of the pull request. Write it from the final `git diff <base>...HEAD` and test output, never from the initial plan: plans state intent, diffs show reality. Use `origin/main` for `<base>`, or the parent layer's branch for an upper stack layer.

The composer agent rewrites this file into the final PR description. It cuts and formats, but does not research. Because it reads little beyond this file, any detail omitted here will be missing from the PR.

## Rules

- **Prioritize completeness over polish.** Use rough sentences or bullets. The composer handles phrasing and cuts.
- **Drop empty sections.** If a field has nothing to say, omit it entirely. Do not write "None" or keep placeholder text.
- **Ground claims in artifacts.** Derive all technical statements, file paths, and snippets directly from the final diff and test execution. Copy identifiers and paths character-for-character so the composer can quote them safely.
- **Follow naming conventions.** Save the output as `handoffs/pr-facts-<topic>.md` in the primary checkout. `handoffs/` is gitignored.

---

### Tier
- Tier: Light | Standard | Deep. Pick the tier the change earns.

### Stack
- Stack goal: <1–2 sentences on the stack's overall goal, and the bottom PR's number if this is an upper layer; omit if not a stack>

### What changed
- Headline: <one sentence: what this PR's net diff does, as you'd tell the reviewer>
- Changes: <the main changes grouped by purpose, with their key paths; not a file-by-file walk>
- Background: <context a reader might mistake for this PR's work: earlier layers or PRs, behavior that already exists, things that never existed on the base>

### Why
- <author rationale and motivation>

### Feedback wanted
- Review focus: <specific areas, questions, or architectural decisions requiring reviewer attention>

### Judgment points
- <path/to/file>: <risk, open decision, or known issue; order items by highest risk first. State known defects here rather than leaving them in the diff>

### Design
- Design points: <architectural choices or subtleties not obvious from the diff alone>
- Plan deviations: <deviations from initial plan or design, if any>

### Visuals
- <each visual `docs/pull-requests.md` requires (a Mermaid block, before/after CLI output from the changed snapshots, a table) or `save <file>` output: one line describing what it shows>
- <interactive explainer, when `docs/pull-requests.md` calls for one: its `save` link and one line on what the reader can do there>

### Verification
- Checks executed: <commands run and output excerpts beyond standard green checks; recorded or redacted, never raw session data>
- Unverified areas: <code paths or scenarios not exercised>
- Test edits: <any modifications to tests, snapshots, or thresholds>

### Links
- Plan: <path or URL of the committed plan; always link a deep-tier plan>
- Issue: <path or URL>
- Artifacts: <path or URL>

### Emphasis
- <free-form instructions to the composer, e.g. "focus on the API migration; keep the refactor brief">
