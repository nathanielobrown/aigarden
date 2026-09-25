---
name: pr
description: Open a reviewable PR end to end — docs synced into the same PR, fact sheet, composer, gh pr create. Invoke when a task should finish as a PR; the session opening the PR runs it.
---

# PR

Open a reviewable PR with the procedure below. The rules it relies on are in `docs/pull-requests.md` (imported below).

## Procedure

1. **Shape the branch**: Follow [Mechanics](../../../docs/pull-requests.md#mechanics), and [Stacked PRs](../../../docs/pull-requests.md#stacked-prs) for a stack. `mise run check` must pass locally.
2. **Sync documentation**: Update `README.md`, `AGENTS.md`, `docs/design.md` and `docs/roadmap.md` wherever the change affects them, and commit the edits on the branch. There is no doc-writer subagent; do it yourself.
3. **Write the fact sheet** following [fact_sheet.md](fact_sheet.md). Write it yourself; you did the work. This repo has no auditor agent. Never hand the job to an agent that knows the work only from a brief: it loses the rationale and the judgment calls.
4. **Compose the description**: Run the Gemini composer headless through pi with [composer.md](composer.md):

   ```bash
   timeout 1800 pi -p --model openrouter/google/gemini-3.8-flash --append-system-prompt .claude/skills/pr/composer.md "Compose the PR description from <fact sheet path>. The diff base is <base>. Write it to <body path>." < /dev/null
   ```

   The composer writes `handoffs/pr-body-<topic>.md`. Keep the `< /dev/null`: without it, `pi -p` waits on input forever.
5. **Review the body** for factual errors only, not style. Check it against the fact sheet and cut anything the fact sheet does not support. If the errors are more than trivial, fix the fact sheet and recompose. The repo is public, so also confirm that no handoff path, local path or private link appears.
6. **Check diagrams by hand**: This repo has no `diagram-check` task, and PR bodies are not in git, so this is their only check before GitHub renders them. For each Mermaid block, confirm `snake_case` node ids, `<br>` line breaks, and quoted labels that contain punctuation.
7. **Submit**: Push the branch once (`git push -u origin <topic>`), then open the PR:

   ```bash
   gh pr create --title "<emoji> <statement>" --body-file <body path>
   ```

   For a stack, `gh stack submit` opens the PRs; then set each layer's title and body with `gh pr edit`.
8. **Recompose on substantial change**: Recompose when the scope, a design point or a stack layer changes, or a new known issue appears. Update the fact sheet first, then rerun step 4 and update the PR with `gh pr edit --body-file <body path>`. Small review fixes do not trigger it.

@../../../docs/pull-requests.md
