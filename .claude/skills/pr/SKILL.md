---
name: pr
description: Open a reviewable PR end to end — docs synced into the same PR, fact sheet, composer, gh pr create. Invoke when a task should finish as a PR.
---

# PR

Open a reviewable PR with the procedure below. The full rules are in `docs/pull-requests.md` (imported below).

## Procedure

1. **Shape the branch**: One branch per task off `origin/main`, in a worktree. Keep history linear by rebasing. Shape the changes into atomic commits with `<emoji> <statement>` subjects. Run `mise run check` until it passes on the final commit; it is the only gate, because no CI runs on PRs.
2. **Sync documentation**: Update `README.md`, `AGENTS.md`, `docs/design.md` and `docs/roadmap.md` wherever the change affects them, and commit the edits on the branch. There is no doc-writer subagent; do it yourself.
3. **Draft the fact sheet**: Write `handoffs/pr-facts-<topic>.md` in the primary checkout from the final `git diff origin/main...HEAD` and test output, never from the plan. Follow [fact_sheet.md](fact_sheet.md).
4. **Compose the description**: Run the Gemini composer headless through pi with [composer.md](composer.md), writing `handoffs/pr-body-<topic>.md`:

   ```bash
   pi -p --model openrouter/google/gemini-3.8-flash --append-system-prompt .claude/skills/pr/composer.md "Compose the PR description from <fact sheet path>. Write it to <body path>."
   ```

5. **Review the body**: Check `pr-body-<topic>` for factual errors against the diff only, not style. Check that the prose fits the tier's word budget (Light ~75, Standard ~300, Deep ~500), that empty sections are omitted, and that no handoff path, local path or private link appears (the repo is public).
6. **Check diagrams by hand**: This repo has no `diagram-check` task. For each Mermaid block, confirm `snake_case` node ids, `<br>` line breaks, and quoted labels that contain punctuation.
7. **Submit**: Push the branch once (`git push -u origin <topic>`), then open the PR:

   ```bash
   gh pr create --title "<emoji> <statement>" --body-file <body path>
   ```

   `gh pr checks` reports no checks, so there is no CI to wait on.
8. **Recompose on substantial change**: when the scope, a design point or a stack layer changes, or a new known issue appears. Update with `gh pr edit --body-file <body path>`. Small review fixes do not trigger it.
9. **Stacked PRs**: Manage stacks only through `gh stack` (v0.1 or later). Never point a PR at another branch by hand. Target 100–400 code lines per PR, and split above 500. Commit review fixes in the owning layer, then run `gh stack rebase --upstack` and `gh stack push`.

Never merge unless directed.

@../../../docs/pull-requests.md
