-- Wheelhouse's Review config: an example to copy and edit.
--
--   mkdir -p ~/.config/wheelhouse
--   cp examples/review.lua ~/.config/wheelhouse/review.lua
--
-- It's a Lua file that returns a table. Wheelhouse reads it with LuaJIT
-- (brew install luajit), so effort and guides can be functions. The hub
-- reads it again whenever the file changes, and `wh review` shows what it
-- read or what's wrong with it. docs/review.md describes every key.

return {
  -- The repos to review. A PR in one of these that requests you by name
  -- becomes a Review docket. `checkout` (required) is your clone of the repo.
  -- Each reviewer works in its own worktree made from it, so your own
  -- working copy isn't touched.
  repos = {
    ["acme/api"] = {
      checkout = "~/code/acme-api",
      -- Notes for every brief on this repo: how to build and test, what to
      -- watch for.
      notes = [[
Run `just test` for the unit tests and `just lint` before a finding about style.
Migrations live in db/migrations/ and must be reversible.
]],
      -- This repo's own effort and guides win over the ones further down.
      -- effort = { review = "high" },
      -- guides = { review = "Check every new handler for an auth check." },
    },
    -- ["acme/storefront"] = { checkout = "~/code/acme-storefront" },
  },

  -- One to four reviewers, each blind to the others. Give an agent name
  -- ("claude" or "codex"), or a table to name a reviewer, pass it flags, or
  -- give it its own review guide and effort. The first is the primary: the
  -- checker works in its worktree. A name is a-z, 0-9 and -, starts with a
  -- letter, is at most 12 long, and isn't "check".
  --
  -- Leaving `reviewers` out means { "codex" }, so set it.
  reviewers = {
    "claude",
    -- A second, different view. Codex needs the codex CLI on your PATH.
    -- { name = "codex", agent = "codex" },
    -- { name = "claude-2", agent = "claude", guides = { review = "Read the tests first." } },
  },

  -- The checker merges the reviewers' drafts, checks every finding, and
  -- rates the merge risk. "claude" or "codex", or a table:
  -- checker = { agent = "claude", extra_args = { "--model", "opus" }, effort = "high" },
  checker = "claude",

  -- "ask": a checked review waits for you to Post, Preview or Dismiss it.
  -- "auto": the checker posts it. `wh review post-mode ask|auto|config`
  -- overrides this without editing the file.
  post = "ask",

  -- How often to check GitHub for PRs that request you, in seconds (at
  -- least 30).
  poll_seconds = 180,

  -- Rounds running at once. A round waiting on you doesn't count.
  max_rounds = 3,

  -- Effort: "low", "medium", "high", "xhigh" or "max", or nil for the
  -- agent's own setting. A reviewer's own effort wins, then the repo's, then
  -- this one.
  --
  -- A function gets the PR: repo, pr, title, author, round, additions,
  -- deletions, lines, files, labels, changed_files, and touches(paths), true
  -- when a changed file's path contains any of them. On later rounds it also
  -- gets delta_lines, last_verdict, last_blocking and last_merge_risk. The
  -- checker's also gets drafted and drafted_blocking: the findings in the
  -- reviewers' drafts, and how many of them block.
  effort = {
    review = function(pr)
      if pr.touches({ "db/migrations/", "auth/" }) then return "xhigh" end
      if pr.round > 1 and pr.delta_lines and pr.delta_lines < 50 then return "medium" end
      return pr.lines < 150 and "medium" or "high"
    end,
    check = function(pr)
      if (pr.drafted_blocking or 0) > 0 then return "high" end
      return "medium"
    end,
  },

  -- Guides go into the briefs. Each is instructions, a file (a path that
  -- starts with / or ~, which must exist), "bundled" (style and merge_risk
  -- ship one), false for none, or a function of the PR returning one of these.
  guides = {
    -- What to look for. Without one, the reviewers get a short default.
    -- review = "~/.config/wheelhouse/guides/review.md",

    -- How the posted review reads. "bundled" is the default.
    style = "bundled",

    -- The merge-risk rubric. "bundled" is the default; false turns merge
    -- risk off.
    merge_risk = "bundled",

    -- More instructions for the checker.
    -- check = "Drop a finding unless it points at a line in the diff.",
  },

  -- Folders put first on the agents' PATH. Agents already get your shell's
  -- PATH; add a folder here only for a tool that's still missing.
  -- path = { "~/.local/bin" },

  -- Package caches the agents may write to. `extra_caches` adds to the
  -- defaults (Go, npm, pnpm, uv, Playwright, Cargo); `caches` replaces them.
  -- extra_caches = { "~/.gradle/caches" },

  -- How each agent starts. `program` is the binary, `extra_args` adds flags,
  -- and `args` replaces the default flags (Claude Code's is
  -- --permission-mode auto).
  -- agents = {
  --   claude = { extra_args = { "--model", "opus" } },
  --   codex = { program = "/opt/homebrew/bin/codex" },
  -- },
}
