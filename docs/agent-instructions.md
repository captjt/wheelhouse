# Agent instructions

Agents learn about Wheelhouse from two places: a few lines in your global
agent instructions, and the `wheelhouse` skill they load when they need the
details.

1. Paste the block below into both of these files (create either one if
   it's missing):
   - `~/.claude/CLAUDE.md`, for Claude Code
   - `~/.codex/AGENTS.md`, for Codex
2. Install the skill the block points to:

   ```sh
   wh skill install
   ```

   With a shared `~/.agents/skills` folder, it writes the skill there and
   links it into `~/.claude/skills` and `~/.codex/skills`. Without one, it
   goes in `~/.claude/skills/wheelhouse`, for Claude Code only; make
   `~/.agents/skills` first to give Codex the skill too. `wh skill show`
   prints the skill, and `wh skill remove` takes it out.

The lines apply only inside a Rex pane (`REX_SESSION` is set), so an agent
started anywhere else ignores them.

## The block

```markdown
## Wheelhouse

In a Rex pane (REX_SESSION is set), you're on a Wheelhouse docket, and `wh`
reaches the user and other agents. Load the wheelhouse skill for how.

- Start with `wh brief`: your docket's intent, its Linear issue, and what
  was said lately. Follow the intent; when the goal or scope really
  changes, update it (`wh intent D-N - < intent.md`).
- You need a decision and the user may be away, or they should know
  something later: `wh send --to me`, with your recommendation first as an
  `--option`, and what you'll do meanwhile.
- You've built something running locally for the user to see or approve:
  `wh share PORT`, never a localhost link (they may be on their phone).
- You made something the user should be able to open later (a report, a
  page, a design, a screenshot): `wh attach FILE|URL --title "…"`, so it
  stays with the docket after `.tmp/` or a scratch folder is gone.
- A "[wheelhouse] Message #N" arrives: answer it with `wh reply N`, only
  with substance.
- You finish a task: end with a short summary of what changed, how it was
  checked, and what's left. It goes on the docket's record as the turn.
```
