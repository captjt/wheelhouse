# Starting a team

How the first testers get a workspace running: one person runs the
workspace hub, and every engineer joins from their own Mac.
[teams.md](teams.md) is the design. Until the branch merges, build from
`feat/teams`.

## What you need

- **Everyone:** a Mac with Wheelhouse installed from source (the README's
  [Requirements](../README.md#requirements) and [Install](../README.md#install)),
  Rex or Herdr, and Tailscale signed in on that Mac (a personal device,
  not a tagged one: Tailscale gives a tagged device no login).
- **Whoever runs the workspace hub:** a Linux box or VM on the tailnet
  with Docker (or systemd and a Rust toolchain), or an always-on Mac.
  It needs Tailscale with HTTPS turned on for the tailnet.
- **The network:** either everyone is on one tailnet, or the workspace's
  machine is shared into each tester's own tailnet (Tailscale's admin
  console: the machine's **Share…**). Either way, each tester must be able
  to open the workspace's `https://….ts.net` address.

## 1. Run the workspace hub (one person, once)

On a Linux box or VM, from a checkout of the repo:

```sh
docker build -f examples/workspace/Dockerfile -t wheelhouse-workspace .
docker run -d --name wheelhouse-workspace --restart unless-stopped \
  -p 127.0.0.1:7700:7700 -v wheelhouse-workspace:/var/lib/wheelhouse-workspace \
  wheelhouse-workspace
sudo tailscale serve --bg 7700
docker logs wheelhouse-workspace     # the setup code: whs1.…
```

Without Docker, use `examples/workspace/wheelhouse-workspace.service`
(its head has the steps). On an always-on Mac: `wh up --workspace`, then
`tailscale serve --bg 7700`, and the setup code is in
`~/Library/Logs/wheelhouse-workspace.log`.

`tailscale serve status` shows the address, like
`https://ws.tail1234.ts.net`. The workspace hub answers no request
without a tailnet login, so opening that address in a browser shows a
short JSON answer and nothing else.

## 2. Make the workspace (the first admin)

On the first admin's Mac:

```sh
wh team create Acme --prefix ACME --url https://ws.tail1234.ts.net --setup whs1.…
```

- `--prefix` numbers the shared dockets: `ACME-1`, `ACME-2`, ….
- Your handle defaults to your user name, lowercased (`--handle` to pick
  one). Teammates see it on everything you write.
- Your Tailscale login comes from your Mac's Tailscale; if Wheelhouse
  can't read it, pass `--login you@example.com`. It must be the login the
  workspace sees, or the create is refused.

## 3. Invite each engineer

```sh
wh team invite sam            # --admin to make them an admin; --hours 48 for longer
```

It prints a code (`whi1.…`). Send it to Sam however you like; it works
once, for 24 hours, and only as `sam`. The code's secret never reaches
the workspace hub: it proves to every hub that Sam's key is the one this
invite admitted.

## 4. Join (each engineer)

On their own Mac, with Wheelhouse running (`wh status`):

```sh
wh team join whi1.…
wh team                       # the workspace, connected, its members
```

If the join says the login isn't the one Tailscale sees, pass
`--login` with the login shown in Tailscale's menu.

Then:

- Tell your agents: `wh skill install` again, so they know about shared
  dockets and teammates' messages.
- In the app, the workspace shows in the sidebar under its name.

## 5. Work on something together

```sh
wh team share D-12                 # one of yours, with its history → ACME-n
wh new --team "Rate limits"        # or a new one, straight on the workspace
wh team people ACME-1 sam --owner  # add Sam as an owner (or leave off --owner)
```

- Start an agent on it from your Mac (`wh start --docket ACME-1`, or
  **Add an agent** on its page), and so does your teammate from theirs.
  Each agent's `wh brief` lists the other's.
- `wh send --to @ACME-1 "…"` from an agent reaches the teammate's agents.
  The first message from a teammate's agents waits in your Inbox:
  **Deliver**, **Drop**, or **Let Sam's agents through** on that docket.
- An agent's question (`wh send --to me`) reaches its own person and every
  owner. Whoever answers first, it stands.
- Only owners change a shared docket's intent and status.

## What stays on your Mac

Your panes and what's on their screens, permission prompts and what you
approved (the team sees only that you approved one), your credentials,
and every docket you haven't shared. `wh team trust ACME-1 --private-prompts on`
keeps your prompts on a docket to yourself too. An entry that looks like
it holds a secret is held in your Inbox before it leaves: `wh team send N`
or `wh team drop N`.

## When something's off

- **`wh team` says not connected:** the workspace hub is down or
  unreachable. Your writes wait in the outbox and go when it's back.
  `wh team sync` tries now and says why not.
- **"the Tailscale login … isn't yours":** you're reaching the workspace
  as another tailnet user, or from a tagged device.
- **"Not followed" on a shared docket:** your hub refused something the
  workspace hub served (a changed entry, a fork, a key no member issued).
  Nothing was taken in; tell the workspace's admin, and keep the hub's
  log (`wh logs`).
- **`wh verify ACME-1`** checks your copy against every member's keys.

## Feedback

Leave what you find on D-187 (`wh note D-187 "…"`), or tell Jordan.
