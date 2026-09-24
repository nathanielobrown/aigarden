# Pull requests

Reviewer attention is the scarcest resource in this project. AI writes most of the code, so the PR description must orient the reviewer before they open the diff.

aigarden is a public, single-user repository. Everything in a PR title, body, or comment is world-readable.

## Mechanics

Use `git` for branches and commits, `gh stack` for stacks, and `gh` for pull requests. Sessions are non-interactive; never pass `-i` flags.

**The flow:**

- Create one branch per task from `origin/main` in a worktree. Keep commits atomic, using an `<emoji> <statement>` subject (see `git log` for conventions).
- Keep history linear. When `main` advances, rebase onto `origin/main`. Never merge `main` into your branch.
- Run `mise run check` until it passes. Push once, only when the branch is ready for review.
- Update affected docs (`README.md`, `AGENTS.md`, `docs/design.md`, `docs/roadmap.md`) on the branch so changes land together. The authoring agent handles this sync directly; there is no doc-writer subagent.
- Open the PR with `gh pr create --title "<emoji> <statement>" --body-file <file>`. Omit issue or PR numbers from the title. PRs land by squash, so this title becomes the commit subject on `main` alongside an appended `(#N)`. Mark a PR as a draft only when it is not ready for review.
- Address review comments with new commits. Fixup commits do not need autosquashing because the final squash absorbs them.
- Land only when directed; see [Landing](#landing).

**Direct commits to `main`:** Most of this repo's history, including features and release bumps, went straight to `main` without a PR. From now on, only trivial edits (like a typo or config fix) may go straight to `main`. Every substantial change requires a branch and a PR, and agents always open a PR. Pushing a modified `version` in `Cargo.toml` to `main` publishes a release (see [CI and checks](#ci-and-checks)).

## Stacked PRs

Stacks are rare. When needed, use GitHub native stacks via the `gh stack` extension (v0.1 or later). Do not use Graphite. **Never point a PR at another branch manually.** Only `gh stack` may create a PR with a base other than `main`.

- **Layer order:** Place enabling refactors at the bottom, the core change in the middle, and incidental improvements on top.
- **One concern per PR:** If you cannot state the change in one sentence, split it. Each layer must build, pass `mise run check`, and include its own tests.
- **Plan layers upfront:** Design layers before writing code so refactors do not break intermediate gates.
- **Size:** Target 100–400 lines of changed code per PR; split above 500. Tests, snapshots, comments, and docs do not count toward this limit. Mechanical changes (such as renames) may exceed it if explained in the description.
- **Descriptions:** Create one fact sheet and one body per PR. State the overall stack goal in the bottom PR (one or two sentences); upper PRs should reference the bottom PR instead. Do not write "part n of m"; GitHub renders the stack map.
- **Review fixes:** Commit fixes to the layer owning the change, then run `gh stack rebase --upstack` and `gh stack push`. Never rebase or amend layers using plain git; gh-stack bug #193 duplicates commits into upper layers.
- **Agent commands:** Run non-interactive `gh stack` subcommands only (`view --json`, `submit --auto`, explicit branch names). Never run bare `modify`.

## What makes a PR done

A completed PR meets three requirements:

1. **Working and verified:** `mise run check` passes. The description highlights what remains unverified rather than listing passing gates.
2. **Docs updated:** All relevant documentation changes are included in the PR.
3. **Reviewer context:** A reviewer understands the change without reading the diff first and knows where human judgment is required. See [Writing PR descriptions](#writing-pr-descriptions).

## Writing PR descriptions

The diff shows *what* changed; the description explains *why* and highlights choices requiring human judgment. PR descriptions orient the reviewer rather than acting as permanent documentation. Keep lasting rationale in `docs/design.md`, code comments, and tests.

### Authoring process

1. **Write the fact sheet:** The authoring agent (usually Claude) generates `handoffs/pr-facts-<topic>.md` in the primary checkout from `git diff origin/main...HEAD` and test outputs, never from the task plan. Follow `.claude/skills/pr/fact_sheet.md`. `handoffs/` is gitignored; do not commit it or reference its paths in the PR.
2. **Compose the body with Gemini:** Run Gemini headlessly via pi:

   ```bash
   pi -p --model openrouter/google/gemini-3.8-flash --append-system-prompt .claude/skills/pr/composer.md "<instruction naming the fact sheet and output paths>"
   ```

   The composer writes `handoffs/pr-body-<topic>.md` following `.claude/skills/pr/composer.md`. It may only inspect the repository to verify claims against the diff or quote code verbatim; it must not introduce topics absent from the fact sheet. Gemini produces clearer prose than Claude.
3. **Verify facts:** The submitting agent checks the body against the diff for factual accuracy (not style), manually reviews any Mermaid syntax (see [Diagrams](#diagrams)), and opens the PR using `gh pr create --body-file <file>`.
4. **Recompose on substantial changes:** Regenerate the body if the scope changes, design decisions shift, new defects appear, or the stack structure changes. Small review fixes do not require recomposition. Apply updates via `gh pr edit --body-file <file>`.

### Body layout and word budgets

`.claude/skills/pr/composer.md` governs layout, section rules, and word budgets:

```text
<what changed and why: 2–3 sentences, no heading>

## Needs your judgment
Opens with the kind of feedback wanted. Then known issues, open decisions and review
questions, each with its file, ordered by risk.

## How it works
One visual (diagram, before/after output, or save link) plus the design points the diff
doesn't make obvious. Plan deviations go here, and only if there are any.

## Verification
Only evidence beyond the standard green checks: manual runs, before/after output, what
went unverified, and any edit to tests, snapshots or thresholds.

<footer: links to plan, issue, artifacts>
```

- **Light PRs** contain only the opening paragraph. Omit empty sections entirely.
- **Word budgets for prose:** ~75 words for Light, ~300 for Standard, and ~500 for Deep. Code blocks, diagrams, and `<details>` tags do not count toward budgets.
- **Rationale:** Stating desired feedback correlates strongly with merge rates (odds ratio 1.72, arXiv 2602.14611). Concise, structured bodies speed up reviews.

### Pitfalls to avoid

- Walking through files line by line to explain syntax instead of intent.
- Drowning key judgment calls inside mechanical diff summaries.
- Summarizing the initial plan rather than what landed in the diff.
- Leaving placeholder text in empty sections.
- Concealing known issues or choices in the diff instead of listing them under "Needs your judgment".
- Pasting full gate output or writing "ran tests". Note exceptions instead.
- Using relative Markdown links, which break on github.com. Use full URLs or backticked paths.

## Visuals

Visuals communicate changes faster than diffs.

- **New or altered flows** (such as the rule engine, `cog`, `mv`, or config loading) require a Mermaid diagram.
- **CLI output changes** require before/after excerpts in a code block. Quote relevant lines from the changed `insta` snapshots rather than dumping the full file.
- **Performance changes** require a before/after comparison table.
- **Hosting:** Run `save <file>` to upload an asset and print a Markdown snippet (inline image for graphics, a link otherwise).

### Diagrams

Keep diagrams focused on a single question and split any graph exceeding 20 nodes. Use `flowchart` for pipelines, `sequenceDiagram` for call order, `stateDiagram-v2` for lifecycles, and before/after pairs for refactors.

Because the repository lacks a `diagram-check` task, validate Mermaid syntax by hand before submitting: check for `snake_case` node IDs, `<br>` for line breaks, and quotes around labels containing punctuation. For lasting architecture, update the flowchart in `docs/design.md`; PR diagrams go stale after merge.

## Submission checklist

- [ ] `mise run check` passes locally on the final commit
- [ ] Git history is linear on `origin/main` with atomic commits
- [ ] Stacks use native `gh stack`, planned layers, and 100–400 code lines per PR
- [ ] Docs are updated in the PR
- [ ] Fact sheet generated from diff and test runs (`handoffs/pr-facts-<topic>.md`)
- [ ] Body composed by Gemini Flash via `pi` using `.claude/skills/pr/composer.md`
- [ ] Body verified against diff, matches tier word budget, and omits empty sections
- [ ] Visuals included where required; Mermaid validated manually
- [ ] Public repo safety: no handoff paths, local file paths, or private URLs in the body
- [ ] Created via `gh pr create --body-file <file>`

## CI and checks

Pull requests do not run CI checks. The single workflow, `.github/workflows/release.yml`, runs on pushes to `main` and publishes whenever `Cargo.toml` contains an unreleased `version`. Local validation via `mise run check` on the final commit is the sole gate. Because `gh pr checks` reports nothing, agents do not need to poll for status.

## Landing

Every PR lands by squash merge, and only when directed. Use `gh pr merge --squash` (or the GitHub UI) for single branches, and `gh stack merge --squash` for stacks. Merging a mid-stack PR also merges all underlying layers, and GitHub retargets the next layer to `main`.

Merging a PR that bumps `version` in `Cargo.toml` immediately triggers a release.

Repository settings enforce squash merges only, enable `delete_branch_on_merge`, and configure commit messages with title `PR_TITLE` and a `BLANK` body. Deleting the head branch on merge triggers GitHub to retarget child PRs to `main`, preventing stranded base branches.
