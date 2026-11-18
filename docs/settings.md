# Settings

Settings, in the web app and in Wheelhouse.app, shows what Wheelhouse does
on its own, as sentences, and lets you change it: gates, agents, each
repo's notes and checks, and the guides and briefs agents are given.

## Standing orders

The first thing Settings shows: each rule that starts work without you,
and how that work runs.

- *When a PR in acme/api requests you, Review it with Claude Code and
  Codex, check with Claude Code, and post it if it approves and its merge
  risk is low, or else ask you.*
- *In acme/docs, a checked review posts itself.*
- *When an ENG issue assigned to you is labeled `agent`, build it with
  Claude Code, check with Codex, and ask you at plan and open.*
- *Before a PR in acme/api opens or a round pushes, run `test` and `lint`
  at that commit myself, and hold it until `test` passes.*

They're worked out from the config, so they always say what will happen.

## What you can change

| Where | What |
| --- | --- |
| Gates | Each of Build's gates: ask, or go ahead (`start`, `start_cycle`, `plan`, `preview`, `open`, `address`, `push`) |
| Agents | Build's builder, checker and pre-reviewers; fix rounds; Review's reviewers and checker |
| Repos | Notes for agents (the repo's rules), the base branch, how much of the plan goes on the PR, witnessed checks ([verify.md](verify.md)), and the repo's review guide |
| Guides & briefs | The Markdown each agent is given: edit, compare with the built-in, reset, or write a guide of your own |
| Review | Who posts a checked review: Ask, Auto, or Auto when… (a rule, [review.md](review.md#who-posts)), for every repo and for each one; and pausing |

## Standing decisions

A decision you answered on one docket can stand for its repo: **Make
standing** on the decision (or `POST /api/dockets/D-6/standing {key}`)
adds it to the repo's notes, under "Standing decisions", with where it was
decided:

```
- How many retries? 3: To match the API docs. (Decided on D-6, November 18.)
```

The notes go into every later brief in that repo, the builder's,
checker's and reviewers' alike, and Settings → Repos shows and edits them.
The docket keeps a `standing` entry saying you made it so.

## Where it's kept

Values go in `~/.config/wheelhouse/settings.json` (`$WH_SETTINGS`),
shaped like the Lua configs and laid over them as they're read:

```json
{
  "build": {
    "gates": { "open": "auto" },
    "repos": { "acme/api": { "notes": "Run just check before you commit.", "checks": { "test": "cargo test" } } }
  },
  "review": {
    "repos": { "acme/api": { "guides": { "review": "~/.config/wheelhouse/guides/api-review.md" } } }
  }
}
```

A value here wins over `build.lua` or `review.lua`; removing it (Settings'
**Use build.lua's**, or `null` through the API) brings back the file's.
Review's posting rule (`post`, and each repo's) is replaced whole, not key
by key, and setting it ends a `wh review post-mode` override. A function
in `review.lua` stays a function until you set something here.
A check `build.lua` lists can be turned off here too: Settings writes it
as `false`.
Every change is checked by the same code that reads the Lua, so one that
wouldn't load is refused with the reason, and nothing is written.

Lua keeps what's code or reaches outside: effort functions, agents'
programs and arguments, PATH, caches and the wall. Settings won't change
those; edit the Lua.

Guides and briefs are files: a built-in one is replaced by a file of the
same name under `~/.config/wheelhouse/` (`guides/`, `briefs/build/`,
`briefs/review/`). Reset sets yours aside. A guide of your own can't be
removed while a config still names it. Briefs use `{{VARIABLES}}` the hub
fills in; a broken one changes what every agent is told, so compare
before you save.

Whatever a save replaces, settings.json or a guide or brief, is kept in
`~/.config/wheelhouse/.history/`, stamped with the time.

## In the API

- `GET /api/settings`: the standing orders, the config in force, what's
  set here (`set`), errors, and every guide and brief.
- `POST /api/settings` with a patch: `{"build": {"gates": {"open":
  "auto"}}}`.
- `GET /api/settings/doc?id=guide/pr-style.md`, `POST /api/settings/doc
  {id, text}`, `POST /api/settings/doc/reset {id}`.

Settings changes go through the same API as everything else, so they share
its trust boundary (docs/security.md).
