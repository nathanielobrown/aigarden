---
name: pr
description: Open a reviewable PR end to end — docs synced into the same PR, fact sheet, composer, gh pr create. Invoke when a task should finish as a PR; the session opening the PR runs it.
---

# PR

Open a reviewable PR with the procedure below. The full rules are in `docs/pull-requests.md` (imported below).

## Procedure

1. **Shape the branch**: One branch per task off `origin/main`, in a worktree. Keep history linear by rebasing. Shape the changes into atomic commits with `<emoji> <statement>` subjects. Run `mise run check` until it passes on the final commit; it is the only gate, because no CI runs on PRs.
2. **Sync documentation**: Update `README.md`, `AGENTS.md`, `docs/design.md` and `docs/roadmap.md` wherever the change affects them, and commit the edits on the branch. There is no doc-writer subagent; do it yourself.
3. **Write the fact sheet**: Write `handoffs/pr-facts-<topic>.md` in the primary checkout, a verbose draft of the PR that the composer revises. It comes from the final `git diff <base>...HEAD` and test output, never from the plan. `<base>` is `origin/main`, or the parent layer's branch for an upper stack layer. Follow [fact_sheet.md](fact_sheet.md).
   - Write it yourself; you did the work. This repo has no auditor agent. Never hand the job to an agent that knows the work only from a brief.
   - For a change that is hard to grasp from text and one diagram, build an interactive HTML explainer, upload it with `save`, and add the link to the fact sheet's Visuals (`docs/pull-requests.md`).
4. **Compose the description**: Run the Gemini composer headless through pi with [composer.md](composer.md), writing `handoffs/pr-body-<topic>.md`:

   ```bash
   timeout 1800 pi -p --model openrouter/google/gemini-3.8-flash --append-system-prompt .claude/skills/pr/composer.md "Compose the PR description from <fact sheet path>. The diff base is <base>. Write it to <body path>." < /dev/null
   ```

   Keep the `< /dev/null`: without it, `pi -p` waits on input forever.

5. **Review the body**: Check `pr-body-<topic>` for factual errors only, not style. Check it against the fact sheet and cut anything the fact sheet does not support. Check that the prose fits the tier's word budget (Light ~75, Standard ~300, Deep ~500), that empty sections are omitted, and that no handoff path, local path or private link appears (the repo is public).
6. **Check diagrams by hand**: This repo has no `diagram-check` task. For each Mermaid block, confirm `snake_case` node ids, `<br>` line breaks, and quoted labels that contain punctuation.
7. **Submit**: Push the branch once (`git push -u origin <topic>`), then open the PR:

   ```bash
   gh pr create --title "<emoji> <statement>" --body-file <body path>
   ```

   `gh pr checks` reports no checks, so there is no CI to wait on.
8. **Recompose on substantial change**: when the scope, a design point or a stack layer changes, or a new known issue appears. Update the fact sheet first. Update the PR with `gh pr edit --body-file <body path>`. Small review fixes do not trigger it.
9. **Stacked PRs**: Manage stacks only through `gh stack` (v0.1 or later). Never point a PR at another branch by hand. Target 100–400 code lines per PR, and split above 500. Commit review fixes in the owning layer, then run `gh stack rebase --upstack` and `gh stack push`.

Never merge unless directed.

@../../../docs/pull-requests.md
