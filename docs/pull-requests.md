# Pull requests

Reviewer attention is the scarcest resource in this project. AI writes most of the code, so the PR description must orient the reviewer before they open the diff.

aigarden is a public, single-user repository. Everything in a PR title, body, or comment is world-readable.

This document holds the rules for branches, stacks, visuals, CI and landing. Each part of writing a PR description is defined in one file beside the `pr` skill:

| What | Where |
| --- | --- |
| The steps to open a PR, and who writes the fact sheet | `.claude/skills/pr/SKILL.md` |
| The fact sheet's format and how to write it | `.claude/skills/pr/fact_sheet.md` |
| The description's layout, word budgets and style | `.claude/skills/pr/composer.md` |

## Mechanics

Use `git` for branches and commits, `gh stack` for stacks, and `gh` for pull requests. Sessions are non-interactive; never pass `-i` flags.

- Create one branch per task from `origin/main` in a worktree. Keep commits atomic, using an `<emoji> <statement>` subject (see `git log` for conventions).
- Keep history linear. When `main` advances, rebase onto `origin/main`. Never merge `main` into your branch.
- Run `mise run check` until it passes. Push once, only when the branch is ready for review.
- Format the PR title as a commit subject: `<emoji> <statement>`. Omit issue or PR numbers from the title. PRs land by squash, so this title becomes the commit subject on `main` alongside an appended `(#N)`. Mark a PR as a draft only when it is not ready for review.
- Address review comments with new commits in the layer that owns the change. Fixup commits do not need autosquashing because the final squash absorbs them.

**Direct commits to `main`:** Most of this repo's history, including features and release bumps, went straight to `main` without a PR. From now on, only trivial edits (like a typo or config fix) may go straight to `main`. Every substantial change requires a branch and a PR, and agents always open a PR. Pushing a modified `version` in `Cargo.toml` to `main` publishes a release (see [CI and checks](#ci-and-checks)).

## Stacked PRs

Stacks are rare. When needed, use GitHub native stacks via the `gh stack` extension (v0.1 or later). Do not use Graphite. **Never point a PR at another branch manually.** Only `gh stack` may create a PR with a base other than `main`.

- **Layer order:** Place enabling refactors at the bottom, the core change in the middle, and incidental improvements on top.
- **One concern per PR:** If you cannot state the change in one sentence, split it. Each layer must build, pass `mise run check`, and include its own tests.
- **Plan layers upfront:** Design layers before writing code so refactors do not break intermediate gates.
- **Size:** Target 100–400 lines of changed code per PR; split above 500. Tests, snapshots, comments, and docs do not count toward this limit. Mechanical changes (such as renames) may exceed it if explained in the description.
- **Depth:** Keep stacks to 2–5 layers as soft guidance. More than five layers usually spans multiple stories; start a second stack instead.
- **Descriptions:** Each layer gets its own fact sheet and description, written against the layer below.
- **Review fixes:** After committing a fix in its layer, run `gh stack rebase --upstack` and `gh stack push`. Never rebase or amend layers using plain git; gh-stack bug #193 duplicates commits into upper layers.
- **Agent commands:** Run non-interactive `gh stack` subcommands only (`view --json`, `submit --auto`, explicit branch names). Never run bare `modify`.
- **Starting and submitting:** Fast-forward local `main` to `origin/main` before `gh stack init`, which records local `main` as the stack's base. `gh stack submit` opens each PR with a generated title and body, so afterwards set each layer's real title and composed body with `gh pr edit <n> --title "<emoji> <statement>" --body-file <file>`.

## What makes a PR done

A completed PR meets three requirements:

1. **Working and verified:** `mise run check` passes. The description highlights what remains unverified rather than listing passing gates.
2. **Docs updated:** All relevant documentation changes are included in the PR.
3. **Reviewer context:** A reviewer understands the change without reading the diff first and knows where human judgment is required.

The diff shows *what* changed; the description explains *why* and highlights choices requiring human judgment. It orients today's reviewer rather than acting as permanent documentation: keep lasting rationale in `docs/design.md`, code comments, and tests. Claude records the facts in a fact sheet and Gemini writes the prose, because Gemini's prose is easier to read.

## Visuals

Visuals communicate changes faster than diffs. The fact sheet lists each one, and the composer places it.

- **New or altered flows** (such as the rule engine, `cog`, `mv`, or config loading) require a Mermaid diagram.
- **CLI output changes** require before/after excerpts in a code block. Quote relevant lines from the changed `insta` snapshots rather than dumping the full file.
- **Performance changes** require a before/after comparison table.
- **Hosting:** Run `save <file>` to upload an asset and print a Markdown snippet (inline image for graphics, a link otherwise).

### Interactive explainers

Build an interactive HTML explainer when text and a single diagram cannot convey the change. Good candidates:

- Stepping through a rule's findings on real files.
- Filtering a before/after table of recorded output.

Build rules:

- **Self-contained:** One HTML file with inline CSS and JavaScript (`save` warns on relative paths in a single file), or a directory uploaded with `save <dir> --entry index.html`.
- **Recorded or redacted data:** Never live session data, since anyone with the link can open it.
- **Checked in a browser:** It renders without console errors.
- **Kept as a handoff:** Save the source in `handoffs/` next to the fact sheet so a recompose can update and re-upload it.
- **Linked, not relied on:** Upload it with `save` and add the link to the fact sheet's Visuals with one line on what the reader can do there. The PR description must stand on its own without it.

### Diagrams

Draw flows, orderings and state transitions, not the diff or the file tree. Keep diagrams focused on a single question and split any graph exceeding 20 nodes. Use `flowchart` for pipelines, `sequenceDiagram` for call order, `stateDiagram-v2` for lifecycles, and before/after pairs for refactors.

For lasting architecture, update the flowchart in `docs/design.md`; PR diagrams go stale after merge.

## CI and checks

Pull requests do not run CI checks. The single workflow, `.github/workflows/release.yml`, runs on pushes to `main` and publishes whenever `Cargo.toml` contains an unreleased `version`. Local validation via `mise run check` on the final commit is the sole gate. Because `gh pr checks` reports nothing, agents do not need to poll for status.

## Landing

Every PR lands by squash merge, and only when directed. Use `gh pr merge --squash` (or the GitHub UI) for single branches, and `gh stack merge --squash` for stacks. Merging a mid-stack PR also merges all underlying layers, and GitHub retargets the next layer to `main`.

Merging a PR that bumps `version` in `Cargo.toml` immediately triggers a release.

Repository settings enforce squash merges only, enable `delete_branch_on_merge`, and configure commit messages with title `PR_TITLE` and a `BLANK` body. Deleting the head branch on merge triggers GitHub to retarget child PRs to `main`, preventing stranded base branches.
