# The record, v1

Status: draft, 2026-11-17. This is the part of Wheelhouse that's costly to
change once anything is written, so it changes only with a version bump.

A docket is its entries, in order. An entry is never changed or removed. A
correction is a new entry that names the one it replaces. Everything else
(a docket's title, status, agents, what it waits on) is worked out from the
entries, and can be rebuilt from them at any time.

## An entry

A JSON object, as UTF-8:

| Field | Required | Meaning |
| --- | --- | --- |
| `v` | yes | `1` |
| `docket` | yes | The docket's id, such as `D-12` |
| `kind` | yes | What the entry is: see "Kinds" |
| `at` | yes | When the writer wrote it: RFC 3339, UTC, to the millisecond |
| `actor` | yes | Who wrote it: see "Actors" |
| `body` | no | Text for people: a note, a title, an agent's last message |
| `data` | no | An object whose fields depend on `kind` |
| `blobs` | no | Files the entry names, each as `sha256:<hex>` |
| `replaces` | no | The id of an entry this one corrects |

Readers keep fields they don't know and ignore them, so a newer writer's
entries still read.

### Actors

`actor` is an object with `type` and `id`, and may carry more:

| `type` | `id` | More |
| --- | --- | --- |
| `person` | The person's handle | |
| `agent` | `<app>@<host>`, such as `claude-code@local` | `block` (its Rex pane), `session` (the agent's own session id) |
| `hub` | `hub@<host>` | |

The hub fills `actor` in from where a request came from. A caller never
names itself.

## The envelope

Each entry is signed in a [DSSE](https://github.com/secure-systems-lab/dsse)
envelope:

```json
{"payloadType":"application/vnd.wheelhouse.entry+json",
 "payload":"<the entry's bytes, base64>",
 "signatures":[{"keyid":"ed25519:<public key, hex>","sig":"<signature, base64>"}]}
```

- **The signature** is Ed25519 over DSSE's pre-authentication encoding:
  `"DSSEv1" SP len(type) SP type SP len(payload) SP payload`, where each
  `len` is a byte count in ASCII decimal and SP is one space.
- **Exact bytes.** The writer serializes the entry once and signs those
  bytes. The hub serializes the envelope once, compactly, and stores those
  bytes. Nothing is ever re-serialized before it's hashed or checked, so
  there are no canonical-JSON rules.
- **Base64** is standard base64 with padding.
- **One signature** in v1: the actor's. A countersign is its own entry.

## Ids, order, and the chain

- **An entry's id** is `sha256` of its envelope's bytes, written
  `sha256:<hex>`.
- **Order.** The hub gives each entry a `seq` within its docket, from 1,
  with no gaps.
- **The chain.** `chain(1) = sha256(Z ‖ id(1))` and
  `chain(n) = sha256(chain(n-1) ‖ id(n))`, where `Z` is 32 zero bytes and
  `‖` joins the raw 32-byte digests. The chain sits outside what the actor
  signs, because actors sign before the hub orders.
- **The head** is the last entry's chain value. Its first eight hex
  characters name the docket's exact state, as in a PR footer:
  `Docket: D-12 @ 9f2c41ab`.

## Keys

Ed25519. A key's id is `ed25519:` and its 32-byte public key in hex. In v1
the hub holds every key on its machine:

| Key | Signs |
| --- | --- |
| `owner` | What you say: notes, titles, prompts you give an agent |
| `hub` | What the hub saw: an agent opening, a turn ending, a pane closing |
| `agent-<app>@<host>` | What an agent says through `wh` |

## Marks

Every entry shows how it's known, worked out from its actor and kind:

| Mark | When |
| --- | --- |
| **witnessed** | The hub wrote it: it saw this happen. |
| **checkable** | A `link` or an `observed` read from a source anyone can read again (later). |
| **said** | A person or an agent wrote it. True if you trust them. |

## Kinds

v1 writes these. More come with the protocols (`question`, `decision`,
`approval`, `artifact`, `link`, `check`, `verdict`, and the rest from the
concept).

| Kind | Writer | `data` |
| --- | --- | --- |
| `open` | any | `protocol` (`agent` or `none`), `title_source` (`pane`, `prompt`, `person`), and for an agent `agent`: `{app, host, block, rex_session, pane, cwd}` |
| `claim` | hub | An agent session holds the docket: `session`, `source` (`startup`, `resume`, `clear`, `compact`, `fork`), `agent` as in `open` |
| `note` | person, agent, hub | `via`: `prompt` (you typed it to the agent), `message` (another agent's message, delivered), or absent (a note on the docket) |
| `turn` | hub | An agent's turn ended. `body` is its last message, when its hook sent one. `state` is what the agent reported after the turn (`done`, `idle`) |
| `observed` | hub | Something the hub saw in Rex: `what` (`blocked`, `error`), `kind`, `record` |
| `set` | person, hub | Any of `title`, `title_source`, `status` |
| `release` | hub | The session no longer holds the docket: `reason` |

Status is one of `todo`, `in_progress`, `done`, `dropped`.

## Rebuilding a docket

Applying entries in order, from nothing:

- `open` sets the title (its `body`), the protocol, the status
  (`in_progress` for an agent, otherwise `todo`), and the agent.
- `set` changes what it names.
- `claim` sets the session and the agent.
- `note` with `via: prompt` counts a prompt and is the last prompt.
- `turn` counts a turn and is the last turn.
- `release` ends the session.
- Every entry moves `updated_at` and the head.

## Checking a docket

`wh verify` checks, for every entry in order:

1. `seq` runs 1, 2, 3, … with no gaps.
2. The id is the sha256 of the stored envelope bytes.
3. The chain value follows from the one before.
4. The envelope's `payloadType` is the entry type above, and its one
   signature checks against a known key.
5. The payload is an entry for this docket.

A changed byte fails 2 (or 4, if the id was recomputed). A removed or
reordered entry fails 1 or 3.

## Export

`wh export D-12` writes JSON lines that check without the hub:

```
{"wheelhouse":"record/v1","docket":"D-12","keys":[{"keyid":"ed25519:…","name":"owner"}]}
{"seq":1,"id":"sha256:…","chain":"…","envelope":"<the envelope's exact bytes, as a JSON string>"}
```

`wh verify --file D-12.jsonl` checks an export with only the keys it names.
