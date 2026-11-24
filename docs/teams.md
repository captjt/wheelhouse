# Teams

How it works day to day, with pictures: [teams-guide.md](teams-guide.md).
To get a workspace running with your first testers:
[teams-start.md](teams-start.md).

A **workspace** joins each engineer's own Wheelhouse into one. A team
works on **shared dockets**, sees who's on what, people and agents alike,
and its agents work together across machines on one signed record. This
page is the design and the protocol. The decisions behind it are on
D-187's record (2026-11-24).

## The pieces

```
           workspace hub  (wh serve --workspace; Linux or a Mac, no runtime)
           ├── ACME-0      the member log: who's in, their keys, admins
           ├── ACME-1…n    shared dockets, ordered here
           └── blobs       files shared dockets name
              ▲   ▲   ▲    HTTPS on the tailnet (tailscale serve)
              │   │   │    every request signed with a member key
   Jordan's hub  Sam's hub  Priya's hub   (wh serve, on each Mac, as today)
   └─ panes, agents, credentials, approvals: never leave
```

- **Each hub stays its owner's.** Panes, agents, credentials, approvals
  and keys stay on the Mac. A member's hub shares only the dockets its
  owner shares, and only what a shared docket carries (below). Outward
  actions (push, post, open a PR) only ever run from the approver's own
  hub.
- **The workspace hub orders.** It's the same binary run as
  `wh serve --workspace`, and it needs no Rex, Herdr, Keychain or
  launchctl. It gives each entry on a shared docket its `seq` and `chain`,
  as a hub does for its own dockets. It signs nothing that anyone relies
  on: every entry is signed by the member, hub or agent that wrote it.
- **The network gets you in; keys prove who wrote what.** The workspace
  hub listens on loopback, and `tailscale serve` publishes it on the
  tailnet. That works for one shared tailnet, and for the workspace's
  machine shared into each teammate's own tailnet. Serve sets
  `Tailscale-User-Login` (and strips any copy a caller sends), and the
  workspace hub checks that the login is the member's. Every request is
  also signed with the member's key, and every entry with its writer's
  key, so the login is only the door.

## Identity

- **A member's key is their hub's `owner` key.** It already signs what
  they say on their own record.
- **Their hub's other keys** (`hub`, and one per agent app) join the
  member log as `key` entries the member signs. An entry signed by one of
  them on a shared docket is that member's hub or agent speaking.
- **Actors are qualified by member.** On a shared docket a person is
  their handle (`sam`), the hub is `hub@sam`, and an agent is
  `codex@sam`. The workspace hub refuses an entry whose actor doesn't
  match its key's member: Sam's hub can't write as Jordan.

## The member log

The workspace's own record, `ACME-0`. It verifies like a docket: signed
entries, chained by the workspace hub. Every hub folds it to the
**roster**: the workspace's name, prefix and id; its members, with each
one's handle, login, key and role; the keys each member issued; and the
open invites.

| Kind | Signed by | `data` |
| --- | --- | --- |
| `workspace` | the creator | `id` (random, 16 bytes hex), `name`, `prefix` (`ACME`), `handle` and `login` of the creator, the first admin |
| `invite` | an admin | `handle`, `role` (`member` or `admin`), `pub` (an invite key's id), `expires` |
| `join` | the joiner | `invite` (the invite entry's id), `handle`, `key` (their member key, which signs this entry), `login`, `name`, `proof` |
| `key` | a member | `keyid`, `use` (`hub` or `agent`), `name` (`hub`, `agent-codex`), `proof`: the named key's signature over `wh-key/v1\n<workspace id>\n<handle>\n<keyid>`, so nobody names a key they don't hold |
| `role` | an admin | `handle`, `role` |
| `remove` | an admin, or the member themselves | `handle`, `reason` |

The rules every hub checks, in order:

- Entry 1 is `workspace`, and only entry 1 is.
- An `invite`, `role` or `remove` of someone else is signed by an admin's
  member key. A member's `remove` of themselves is signed by their own.
- A `join` is signed by the key it names, and that key is new. Its invite
  exists, isn't used and hasn't expired (by the join's `at`), and its
  handle is the invite's. Its `proof` is a signature by the invite key
  over `wh-join/v1\n<workspace id>\n<invite entry id>\n<member key>`.
  The invite's secret never leaves the joiner's Mac, so the workspace hub
  can't hand the invite to a key of its own.
- A `key` is signed by an active member's key, and the key it names is
  new.
- A removed member's keys are revoked at the removal: refused from then
  on, even if the same handle is invited again (that joins with new keys).
  What they signed before it still verifies, against the roster as it
  stood when they signed.
- An entry is taken once: an old one ordered again (a past promotion) is
  refused.
- The workspace hub holds every entry but a `key` to its own clock (within
  5 minutes either way), so a join can't be backdated into an invite's
  life; a `key` may have waited in an offline outbox.
- There's always at least one admin: the last one can't be removed or
  made a member.

### Invites

`wh team invite sam` makes a 32-byte secret. The invite key is the
Ed25519 key with that seed, and its id goes into the `invite` entry. The
**invite code** is `whi1.<base64url of {"u":url,"w":workspace id,"s":seed}>`,
which you hand to Sam however you like. `wh team join CODE` on Sam's Mac
signs the proof with the seed and the `join` entry with Sam's key, then
sends the `key` entries for Sam's hub and agents. An invite expires in 24
hours by default.

### Creating a workspace

`wh serve --workspace` prints a one-time **setup code** at its first start
(and keeps it in its state folder until it's used). `wh team create acme
--url https://… --setup CODE` on the first admin's Mac signs the
`workspace` entry and their `key` entries. A workspace hub with a
workspace refuses another.

## Shared dockets

- **Sharing.** `wh team share D-12` asks the workspace hub for a number
  (`ACME-31`), then sends the docket's first entry, `share`, signed by the
  sharer, its one first owner. It carries the title, status, intent and
  protocol as they were, and names the local docket's history by its
  `head` and entry count. The history itself stays on the sharer's hub:
  it holds their approvals and prompts unredacted. The head lets them show
  it later, checkable. The local docket gets a `set` with `shared_as`, and
  `D-12` stays an alias for `ACME-31` on the sharer's hub.
- **Opening shared.** `wh new --team` opens a docket straight on the
  workspace.
- **Ordering.** The workspace hub orders every entry, from every member,
  in the order they arrive. Two members writing at once both land, one
  after the other, and every hub sees that one order.
- **Writing.** A member's hub signs an entry for a shared docket, puts it
  in its **outbox**, and sends the outbox in order. The workspace hub's
  answer gives its `seq` and `chain`. An entry isn't on the local copy
  until the workspace hub has ordered it, and the apps show what's still
  in the outbox. A hub that's offline keeps sending until it can. Entries
  are signed before they're ordered, so nothing is signed again.
- **Once.** The workspace hub and every member's hub refuse an entry
  that's on the docket already, so an old one (a revoked `trust`) can't
  be sent again to bring it back.
- **Following.** Each member's hub keeps a copy of every shared docket in
  its store, under the workspace's id, with the workspace's `seq` and
  `chain`. It long-polls the workspace hub for changes, takes each new
  entry only if it follows the chain it already has, checks it against
  the roster and the rules below, then folds it.
- **Who's on a docket.** A shared docket has **owners** and **members**,
  any number of each. The sharer is its first owner. A `people` entry
  adds or removes them (`add`/`remove`, `handle`, `role`). An owner may
  change anyone, and any workspace member may add themselves as a member.

### Who may write what on a shared docket

The workspace hub refuses an entry that breaks these, and every member's
hub checks them again as it follows the docket:

- The key is in the roster and its member is active.
- The actor is the key's: a person entry is signed by that person's member
  key, `hub@sam` by Sam's hub key, `codex@sam` by a key Sam issued.
- `entry.docket` is the docket.
- The kind is one Wheelhouse knows, and the envelope is at most 256 KB.
- `share` is entry 1, and only entry 1.
- `intent`, `set`, and `people` that changes owners, removes someone or
  says who answers come from an owner, or their hub or agents.
- `claim`, `release`, `turn`, `observed` and `check` come from a hub: a
  person's claim would read as every hub's own agent.
- `at` isn't more than 5 minutes ahead of the workspace hub's clock.

### What a shared docket carries

| Shared | Redacted | Never leaves your hub |
| --- | --- | --- |
| intent, notes, decisions, messages, artifacts, links, checks, verdicts, claims, releases, prompts, turn summaries | approvals: who approved, in whose agent, and the digest of what | pane screens, permission prompt text, keys, credentials, other dockets |

- **Redacted at writing.** A signed entry can't be redacted later, so on
  a shared docket the hub writes the redacted entry in the first place.
  The full text is a blob kept only on the writer's hub, and the shared
  entry names it by `sha256` in `data.private`. The writer can prove later
  what it was; nobody else can read it.
- **Keeping your prompts private.** A person can turn on "keep my prompts
  private" for a shared docket. Their prompts on it are then written like
  an approval: who prompted which agent, and a digest.
- **Secrets.** Before an entry leaves, the hub checks its text for what
  looks like a secret (private keys, tokens with a known prefix, long
  random strings after `key=`, `token:`, `password`). One that does stays
  in the outbox, held, and in its author's Inbox: **Send anyway** or
  **Drop**.

## Agents across machines

- `wh brief` on a shared docket lists every member's agents on it, with
  addresses that name the member: `sam/codex·web`.
- A message is an entry on the shared docket (`message`, with `to` and
  `from`), so it travels the way every entry does. The hub of the
  agent it's for picks it up as it follows the docket.
- A message from another member's agent waits in the receiving person's
  Inbox: **Deliver**, **Drop**, or **Let Sam's agents through** on that
  docket. That trust is an entry (`trust`, on the shared docket, by the
  person who gave it), and can be taken back.
- A delivered message is marked in the pane as a peer's request, never
  a person's.
- Nothing from another hub can approve, start, stop or hold anything on
  yours.

## Questions on a shared docket

An agent's question (`wh send --to me` from an agent on a shared docket)
goes to its own person and to every owner. The first answer is an entry
signed by whoever gave it; every hub withdraws its own ask when it sees
it. Only a person answers: an owner, someone on the docket when owners let
members answer, or the asking agent's own person. An answer from anyone
else, or from an agent, is never delivered. An owner can let members answer too (`people` with `answer: members`).
Permission prompts go only to the agent's own person.

## Checking

- `wh verify ACME-31` on any member's hub checks the copy: ids, order,
  chain and signatures, with the roster's keys, and the write rules
  above.
- Each hub keeps the head it has seen of every shared docket and of the
  member log. A workspace hub that drops, rewrites or reorders an entry,
  or shows two members different histories, can't extend the chain a hub
  already has. The hub stops following that docket and says so in
  Attention.
- A hub that stopped following a docket says why; `wh team follow ACME-31`
  tries again from where it stopped.

## The workspace hub's API

Everything is under `/team/v1`, JSON, on the workspace hub's one
listener. A member request carries `x-wh-key` (the member key),
`x-wh-time` (Unix ms) and `x-wh-sig`: base64 of an Ed25519 signature over

```
wh-team-req/v1
<workspace id>
<METHOD>
<path and query>
<x-wh-time>
<hex sha256 of the body>
```

The time must be within 5 minutes of the workspace hub's, and a
signature is accepted once.

| Route | Who | What |
| --- | --- | --- |
| `GET /team/v1/hello` | anyone on the tailnet | The workspace's id, name and prefix (or that it has none yet) |
| `POST /team/v1/create` | holder of the setup code | The `workspace` entry and the creator's `key` entries |
| `POST /team/v1/join` | holder of an invite | The `join` entry, then the joiner's `key` entries |
| `GET /team/v1/changes?cursor=N&wait=S` | member | Waits up to S seconds for anything after N: the new cursor, the member log's length, each shared docket's length |
| `GET /team/v1/log?after=N` | member | Member log entries after N |
| `POST /team/v1/log` | member | One member log entry |
| `POST /team/v1/dockets` | member | A new shared docket's number |
| `GET /team/v1/dockets/{id}?after=N` | member | That docket's entries after N |
| `POST /team/v1/dockets/{id}` | member | One entry, for the workspace hub to order |
| `GET /team/v1/blobs/{hex}`, `PUT` | member | A file a shared docket names (at most 25 MB); a `PUT` must hash to its name |

## Running a workspace hub

- **Linux, in a container:** `examples/workspace/Dockerfile` builds an
  image with nothing but `wh`; run it with its state on a volume and its
  port on the host's loopback, then `tailscale serve --bg 7700`. The
  Dockerfile's head has the commands.
- **Linux, under systemd:** `examples/workspace/wheelhouse-workspace.service`,
  with `WH_STATE=/var/lib/wheelhouse-workspace`, then
  `tailscale serve --bg 7700`.
- **GCP, with Terraform:** `examples/workspace/gcp` makes a private VM
  with the hub and Tailscale Serve on it, its state on a snapshotted
  disk, and no way in but the tailnet. [teams-gcp.md](teams-gcp.md) is
  how a team deploys and runs it.
- **A Mac:** `wh up --workspace`, a LaunchAgent beside (never instead of)
  the daily hub, with its own state in `~/.local/state/wheelhouse-workspace`,
  then `tailscale serve --bg 7700`.
- The workspace hub listens on `127.0.0.1:7700` (`WH_WORKSPACE_HTTP`). It
  answers only requests that came through `tailscale serve`, carrying a
  member's login. Its first start prints the setup code, and keeps it in
  `setup-code` in its state folder until a workspace is made.
- Members' hubs must run on user-owned devices: Tailscale gives a tagged
  device no login.

### Testing it

`scripts/team-e2e.sh` (in `just e2e`) runs a workspace hub and two
members' hubs on one Mac, each member reaching the workspace hub through
`scripts/fake-tailscale-serve`, a stand-in that sets their login the way
Serve does. `WS_IMAGE=wheelhouse-workspace:dev scripts/team-e2e.sh` runs
the same steps against the workspace hub in its Linux container.

## Threat model

| Who | Can | Can't |
| --- | --- | --- |
| A teammate's agent | Write on shared dockets as its own member's agent; message your agents, which waits for you unless you trusted them | Type into your panes unasked, approve, start, stop, push or post on your hub, or write as anyone else |
| A member's hub, compromised | Write anything its member could, on dockets it can see | Write as another member, take an invite meant for someone else, or rewrite what's been ordered |
| The workspace hub, misbehaving | Withhold entries, refuse writes, see everything shared | Forge an entry, add a key or a member without an admin's signature, hand an invite to its own key, or rewrite, drop or reorder what a hub already has without that hub noticing |
| A removed member | Keep what they had already copied | Write anything new: their keys are refused from the removal on |
| Anyone else on the tailnet | Reach `/team/v1/hello` | Anything else: no member login, no member key |

What it doesn't stop yet: a workspace hub showing a member a docket that
stops early (withholding is indistinguishable from quiet), and two members
who never compare heads being shown different futures. Cosigned
checkpoints (DESIGN.md, phase 3) close both.
