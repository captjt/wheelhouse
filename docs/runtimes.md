# Runtimes: Rex and Herdr

Wheelhouse's agents run in terminals, and a **runtime** is the terminal
multiplexer that holds them: Rex or Herdr (herdr-core's
`hc`, with the Herdr app and its phone app). The hub follows every runtime
that's on this Mac at once, so an agent in a Herdr terminal and one in a
Rex pane sit side by side in the same dockets, inbox, approvals and
messages. One setting says where the agents Wheelhouse starts itself open.

```sh
wh status                    # each runtime: following, not connected, or not installed
```

| | Rex | Herdr |
| --- | --- | --- |
| Runs the panes | the Rex server | herdr-core (`hc` 0.1.20 or later) |
| Shows them | Rex.app | the Herdr app, and the phone app |
| A pane | a block (`block:…`) | a terminal (`herdr:t2h`) |
| A session | a Rex session | a space (the `herdr.space` label) |
| A tab | a window | a tab (the `herdr.tab` label) |
| An agent's own pane | `REX_BLOCK` | `HC_TERMINAL_ID` |
| Status | OSC 7501 records | the same records, kept by herdr-core |
| Typing a prompt | a bracketed paste, then Enter | `hc send`, which waits out a draft |
| Open in… | `rex focus` and Rex.app | `herdr-app do agent.focus` |

## Where new agents open

`wh start`, Build and Review open their panes on one runtime: Rex unless
you pick Herdr in Settings (Runtimes), or set it yourself:

```json
// ~/.config/wheelhouse/settings.json
{ "runtime": "herdr" }
```

`WH_RUNTIME=herdr` overrides both, for a test hub. The sessions keep their
names on either runtime (`wheelhouse`, `wheelhouse-builds`,
`wheelhouse-reviews`); in Herdr each is a space, and each docket, task or
PR a tab in it. A terminal Wheelhouse opens carries its names as labels
(`wh.session`, `wh.tab`, `wh.pane`), so the hub finds it by name whatever
the Herdr app calls its space and tab, and the app places it in that space
whenever it runs.

Switching takes effect for the next pane. A Build or a Review already
running keeps its panes where they are: every pane is driven by its own
runtime, whatever the setting says now. A pane opened beside one in the
other runtime gets a tab of its own.

## Agents across runtimes

Agents in Rex and in Herdr work together as if they shared one
multiplexer: they message each other, reply, and are messaged by you, and
a message always goes in through the receiving pane's own runtime (a
paste in Rex, `hc send` in Herdr), between its turns.

An address is `session/pane`, a bare pane name, or a pane id:

| Address | Names |
| --- | --- |
| `api/builder` | the pane `builder` in the session `api`, in whichever runtime has it |
| `reviewer` | the pane `reviewer`, in your own session first |
| `herdr:wheelhouse/D-12`, `rex:D-12` | the same, in that runtime only |
| `block:01a2…`, `herdr:t2h` | that pane, by its id |
| `web/t7` | an unnamed Herdr terminal, by its address |

When a session's name is in both runtimes (each has a `wheelhouse`), its
panes' addresses name their runtime (`rex:wheelhouse/D-12`,
`herdr:wheelhouse/D-12`), in `wh whoami`, in a message's "from", and in
tab completion. A name that's in both and not in your own session is
never a guess: the hub refuses it and names each one.

## Agents in Herdr

An agent you start yourself in a Herdr terminal gets a docket, as in Rex,
once it reports its status. `wh` and the hooks find their pane from
`HC_TERMINAL_ID`, so `wh brief`, `wh send` and the rest work the same, and
the [agent instructions](agent-instructions.md) apply when either variable
is set.

- herdr-core starts programs in its own environment. Wheelhouse gives
  each agent it starts your shell's PATH, as it does in Rex.
- An agent too old to report its status never counts as ready: check
  which Claude Code or Codex is first on that PATH.
- Approvals work as in Rex: the hub reads the prompt from herdr-core
  again (`hc get`) and sends a key only if it's the one you saw.
- `wh doctor` checks each runtime that's installed; the one new agents
  open in must be followed.

## The interface

`src/runtime/` is the only part of the hub that reaches a multiplexer.
Each runtime is an adapter with the same verbs, and `runtime::Runtime`
dispatches to it by the pane's id:

| Verb | What it does | Rex | Herdr |
| --- | --- | --- | --- |
| identify | the pane a process runs in; the runtime of an id | `REX_BLOCK` | `HC_TERMINAL_ID`; ids start `herdr:` |
| observe | keep its part of the mirror current, and reconcile the dockets | `rex events`, `session.list`, `program_status` | `hc events`, `hc get` |
| submit | type text and press Enter | `block.event` paste, `send-key enter` | `hc send` |
| keys | press keys by Wheelhouse's names (`enter`, `escape`, `1`) | `rex send-key` | `hc keys` |
| capture | the pane's last lines | `rex capture` | `hc read` |
| records | the status records, read now | `program_status` | `hc get` |
| open | a pane in a tab, or beside another | `rex new`, `rex run` | `hc run --label …` |
| close, rename, focus | | `block close`, `block rename`, `focus` | `hc kill`/`rm`, `hc label`, `herdr-app` |

The mirror (`src/mirror.rs`) holds every runtime's part, each replaced
only by its own follower, with a link per runtime (connected, its error,
installed). A runtime that isn't installed isn't an error: it's looked for
again every minute.

Pane ids carry their runtime: Rex's are its own, and every other runtime's
start with its name. So the record, the store and the apps' JSON keep one
`block` field for any runtime, and history written before Herdr reads the
same. `Live` adds `runtime` for the apps, and `/api/state` lists
`runtimes`.

### Adding a runtime

1. An adapter in `src/runtime/<name>.rs`: its CLI wrapper, its follower
   (a snapshot into `Mirror::replace`, then events), and the verbs above.
2. A `Kind` and a `Runtime` variant in `src/runtime/mod.rs`, its id prefix
   in `of()`, and its pane variable in `pane_from_env()`.
3. A stand-in like `scripts/fake-hc` for `scripts/build-e2e.sh`, and a
   `WH_…` override to wall the real one off in tests and Wheelhouse Dev.

## Tests

- `cargo test` covers the Herdr mirror (terminals as sessions, tabs and
  panes), replacing one runtime's part without touching another's, and
  agents tracked side by side.
- `just e2e` runs the Build end to end twice: once in Rex
  (`scripts/fake-rex`), once in Herdr (`scripts/fake-hc`).
- `WH_HC` and `WH_HERDR_APP` move the hub's `hc` and `herdr-app`, as
  `WH_REX` moves `rex`. The e2e, the screenshots and Wheelhouse Dev set
  them, so none of them reaches your own Herdr.
