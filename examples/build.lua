-- Wheelhouse's Build config: an example to copy and edit.
--
--   mkdir -p ~/.config/wheelhouse
--   cp examples/build.lua ~/.config/wheelhouse/build.lua
--
-- Build runs without this file. It then uses the repos, agents, PATH and
-- caches in review.lua, with the defaults shown here. Like review.lua, it's
-- Lua read with LuaJIT, and the hub reads it again whenever it changes.
-- `wh build` says what's wrong with it, if anything, and `wh build --json`
-- shows what it read. docs/build.md describes every key.

return {
  -- The repos Build works in. Every repo in review.lua is one already, with
  -- its checkout and notes; list a repo here to add one or to change
  -- something. `checkout` is required only for a repo review.lua doesn't list.
  repos = {
    ["acme/api"] = {
      checkout = "~/code/acme-api",
      -- Notes for the builder's and checker's briefs.
      notes = "Run `just test` before committing. Keep handlers thin; logic goes in services/.",
      -- The branch a Build starts from and its PR opens against.
      base = "main",
      -- How much of the plan goes on the PR: "full" (the default),
      -- "summary" (for a public repo whose issues are private: the plan in
      -- the builder's own words), or false (none).
      plan = "full",
    },
    -- ["acme/storefront"] = { checkout = "~/code/acme-storefront", plan = "summary" },
  },

  -- What starts a Build on its own. Without `sources`, Builds start only by
  -- hand: `wh build start D-12`, `wh build start acme/api#45`,
  -- `wh build start ENG-142`, or Build it in the app.
  sources = {
    -- Open GitHub issues assigned to you, in the repos above, with this label.
    github = { label = "agent" },

    -- Linear issues assigned to you (needs `wh setup-linear`), in each team's
    -- repo: with the label they start a Build; in your active cycle they're
    -- offered one; with skip_label they're left alone.
    -- linear = { teams = { ENG = "acme/api" }, label = "agent", cycle = true, skip_label = "no-agent" },
  },

  -- Who builds and who checks: "claude" or "codex", or a table:
  -- checker = { agent = "codex", effort = "high" },
  builder = "claude",
  checker = "claude",

  -- Gates: "ask" waits for you; "auto" goes on without asking.
  gates = {
    plan = "ask",    -- Approve, Revise or Discard the plan
    preview = "ask", -- Looks right or Needs work, when there's something to see
    open = "ask",    -- Open PR, Needs changes or Discard
    address = "ask", -- once it's open: Address it or Not now, on review feedback, failing checks, a moved base
    push = "ask",    -- Push and reply, Needs changes or Drop the round
    -- start = { label = "auto", cycle = "ask" }, -- how a Linear issue starts
  },

  -- How a round brings the branch up to date with its base: "merge" (never
  -- a force-push) or "rebase" (pushed with a lease).
  update = "merge",

  -- Fix rounds after the checker finds something.
  fix_loops = 2,
  -- Decisions a plan may leave open at once. More means it isn't ready.
  max_decisions = 3,
  -- Builds working at once. A Build waiting on you at a gate doesn't count.
  max_tasks = 2,
  -- How often to check GitHub for labeled issues and open PRs, in seconds.
  poll_seconds = 180,

  -- The branch name's prefix, for dockets and GitHub issues. A Linear issue
  -- uses the branch name Linear gives it.
  branch_prefix = "wh/",

  -- The PR as it opens. Every Build PR opens from your GitHub account.
  pr = {
    draft = true,
    -- Reviewers to request, by GitHub login.
    -- reviewers = { "dana", "sam" },
  },

  -- Effort: "low", "medium", "high", "xhigh" or "max", or nil for the
  -- agent's own setting. A builder's or checker's own `effort` wins over
  -- this. A function gets the Build: key, title, repo, and source
  -- ("docket", "github" or "linear").
  effort = {
    build = function(task)
      if task.source == "docket" then return "high" end
      return "medium"
    end,
    check = "high",
  },

  -- Guides go into the briefs: instructions, or a path to a file (starting
  -- with / or ~). A path to a file that doesn't exist is read as
  -- instructions, so check the spelling. `plan`, `build` and `check` add to
  -- the briefs.
  -- `plan_format` and `pr_style` replace the bundled guides/plan-format.md
  -- and guides/pr-style.md.
  guides = {
    -- plan = "Name the test that proves each acceptance item.",
    -- build = "~/.config/wheelhouse/guides/build.md",
    -- check = "Run the full test suite, not only the tests the builder ran.",
    -- pr_style = "~/.config/wheelhouse/guides/pr-style.md",
  },

  -- Builders and checkers run with no MCP servers, so a tracker or GitHub
  -- connector can't reach outward. true turns MCP servers back on.
  wall = { mcp = false },

  -- `agents`, `path` and `caches` work as in review.lua. Leave them out to
  -- use review.lua's.
}
