# A workspace hub on GCP

How a team runs its workspace hub (`wh serve --workspace`) in a GCP
project, with the Terraform module in
[examples/workspace/gcp](../examples/workspace/gcp). [teams.md](teams.md)
is what a workspace hub is; [teams-start.md](teams-start.md) is how
members join one once it's running.

```
  GCP project (its own)                         your tailnet
  ┌──────────────────────────────────────┐
  │ VPC: no ingress, egress 443 + UDP    │
  │ ┌──────────────────────────────────┐ │      jordan's Mac ──┐
  │ │ VM (COS, Shielded, no public IP) │ │                     │ HTTPS, Tailscale
  │ │  tailscale ── Serve :443 ────────┼─┼───── sam's Mac ─────┤ login checked
  │ │      │ Tailscale-User-Login      │ │                     │
  │ │      ▼                           │ │      priya's Mac ───┘
  │ │  wh serve --workspace            │ │
  │ │      127.0.0.2:7700 only         │ │
  │ │  /mnt/disks/wh (data disk) ──────┼─┼── daily snapshots
  │ └──────────────────────────────────┘ │
  │ Cloud NAT · Artifact Registry ·      │
  │ Secret Manager (Tailscale auth key)  │
  └──────────────────────────────────────┘
```

## The one rule

The workspace hub lets a request in by its `Tailscale-User-Login`
header, which Tailscale Serve sets for every caller (and strips from what
callers send). Anything that could reach the hub's port without going
through Serve could write that header itself. A member key is still
needed for everything past `/team/v1/hello`, but the login is the door,
so **nothing but Serve may ever reach port 7700**. The module is built
around that:

| What | Why |
| --- | --- |
| **No public IP, and no ingress rule at all** | Nothing on the internet or in the VPC reaches the VM. Members come in over Tailscale, which dials out. |
| **The hub on `127.0.0.2:7700`, not `127.0.0.1`** | Only processes on the VM reach it: Serve, running beside it. Tailscale runs in userspace mode, where it forwards tailnet TCP for any port Serve doesn't handle on to `127.0.0.1` at that port. A hub there would be reachable at `100.x.y.z:7700`, past Serve. It never forwards to `127.0.0.2`. |
| **The hub's container opens no connections** | It can't reach the metadata server for the VM's token, or anything else: new connections from it are dropped. |
| **Serve's config turns Funnel off** | The hub is never on the public internet, even if someone grants Funnel to its tag. |
| **A tagged Tailscale node** (`tag:wheelhouse-workspace`) | It belongs to no person, its key doesn't expire, and the tailnet policy says exactly who reaches it: members, on 443. It can't open a connection to anyone. The policy's tests refuse to save a change that would open port 7700. |

## What the module makes

- **A VPC of its own**, one small subnet with Private Google Access and
  flow logs, and Cloud NAT to get out. Ingress is denied and logged.
  Egress is limited to TCP 443 and UDP (Tailscale), and the rest is
  denied and logged (`restrict_egress`).
- **One VM** on Container-Optimized OS: Shielded (secure boot, vTPM,
  integrity monitoring), OS Login only, no project-wide SSH keys, no
  serial port, and COS's own automatic updates.
- **Two containers**, each read-only, with every capability dropped,
  `no-new-privileges`, and memory and process limits:
  - `tailscale`, in userspace mode (no `NET_ADMIN`, no `/dev/net/tun`),
    publishing the hub with Serve at `https://NAME.TAILNET.ts.net`.
  - `wheelhouse-workspace`, the hub, by digest, on `127.0.0.2:7700`. The
    VM refuses an image whose `USER` isn't a numeric, non-root `uid:gid`,
    and drops any connection the hub's container tries to open. The image
    ([examples/workspace/Dockerfile](../examples/workspace/Dockerfile))
    is a static `wh` on distroless static (about 2 MB besides `wh`: no
    libc, no shell, no package manager), pinned by digest, as uid 65532.
- **A data disk**, apart from the VM: the member log, every shared
  docket, the files they name, and Tailscale's node key. Mounted
  `noexec,nosuid,nodev`, formatted only if it's blank, snapshotted daily
  (14 days, in the region). Terraform refuses to destroy it. The VM can
  be replaced at any time without losing the workspace or its tailnet
  name.
- **A service account for the VM** that writes logs and metrics, reads
  one image repository and one secret, and nothing else.
- **An Artifact Registry repository** whose tags can't be moved once
  pushed.
- **A Secret Manager secret** for the Tailscale auth key, in the region.
  Terraform makes the secret but never its value: you add the key with
  `gcloud`, so it's never in a plan or the state.
- **SSH through IAP only**, for the people in `ssh_members`, granted on
  this one VM. With `ssh_members` empty there's no SSH rule at all.
- **Optional CMEK** (`kms_key`) for the boot and data disks.

It doesn't make alerting, a GKE or Cloud Run variant, or anything for the
members' own hubs: those stay on their Macs.

## Who does what

A workspace has three kinds of people around it, and they needn't be the
same people. Use groups everywhere, so people come and go without an
apply.

| Who | What they hold | What they need |
| --- | --- | --- |
| **Operators** (`group:wheelhouse-ops`) | The Terraform, the image, SSH to the VM | A role on the GCP project that can make these resources and set their IAM (the project's Owner is simplest, on a project of its own), and edit access to the state bucket. Through `ssh_members` and `image_publishers`: SSH and push. |
| **A tailnet admin** | The tailnet policy, and the auth key | Tailscale admin. Nothing in GCP but adding a secret version (`roles/secretmanager.secretVersionAdder` on the one secret), or an operator does it for them. |
| **Workspace admins** | Who's in the workspace (`wh team invite`, `role`, `remove`) | A Wheelhouse admin role in the member log. No GCP access at all. |
| **Members** | Their own Mac's hub | To be in the tailnet group (`group:wheelhouse`) and in the workspace. No GCP access at all. |

Two things to know when choosing operators:

- **Operators can read everything shared.** It's on the disk they manage.
  They can also stop the hub or run another image. That's the
  "workspace hub, misbehaving" row of [teams.md](teams.md#threat-model):
  they can withhold entries, but can't forge one, add a member, or
  rewrite what members' hubs already have without those hubs noticing.
  Keep the group small.
- **Anyone who can read the project's logs can read the setup code**
  until the workspace is made (below). Make the workspace right after the
  first boot.

## Before you start

- **A GCP project for it alone**, with billing. It keeps the IAM small
  and the blast radius smaller.
- **On the operator's machine:** `gcloud` (signed in), Terraform 1.7 or
  later, Docker with `buildx`, and a checkout of this repo.
- **The tailnet:** MagicDNS and HTTPS certificates on (Tailscale admin
  console → DNS). Then merge
  [tailscale-policy.hujson](../examples/workspace/gcp/tailscale-policy.hujson)
  into the policy file: `group:wheelhouse` with each member's login, the
  tag owned by admins, one grant to `tcp:443` on the tag, and tests
  that fail if anything opens more. If your policy still has the default
  rule that lets everyone reach everything, the hub is still safe (the
  127.0.0.2 binding), but take that rule out or narrow it: the tests
  catch it.
- **Organization policies:** it already satisfies the usual ones (no
  external IPs, OS Login required, Shielded VM required, restricted
  regions if `region` is allowed).

## Deploy

### 1. Configure

```sh
cp -R examples/workspace/gcp/example ~/wheelhouse-gcp   # or into your infra repo
cd ~/wheelhouse-gcp
cp terraform.tfvars.example terraform.tfvars            # project, region, groups
```

Point `source` in `main.tf` at the module where you keep it (a path, or
this repo by git URL and commit). Keep the state in a bucket the
operators share: `backend.tf.example` has the commands. The state holds
no secrets, but it describes the deployment, and two operators must
never apply from two copies of it.

### 2. First apply: everything but the VM

With `image` left out:

```sh
terraform init
terraform apply
```

This turns the APIs on and makes the network, the repository, the
secret, the service account and the data disk.

### 3. The Tailscale auth key

In the Tailscale admin console, **Settings → Keys → Generate auth key**:

- **Reusable:** off. It's spent at the first login.
- **Expiration:** 1 day. It only has to last until step 5.
- **Ephemeral:** off. The node must outlive a reboot.
- **Pre-approved:** on, if your tailnet approves devices.
- **Tags:** `tag:wheelhouse-workspace` (or what you set in
  `tailscale_tags`: exactly those).

Then add it as the secret's version without it touching your shell
history or Terraform:

```sh
read -rs TS_AUTHKEY && export TS_AUTHKEY     # paste, Enter
eval "$(terraform output -raw add_tailscale_auth_key)"
unset TS_AUTHKEY
```

### 4. Build and push the image

From the repo's root, at the commit the team's Macs run:

```sh
REPO=$(terraform -chdir="$HOME/wheelhouse-gcp" output -raw image_repository)
TAG=$(git rev-parse --short HEAD)
gcloud auth configure-docker "${REPO%%/*}"
docker buildx build --platform linux/amd64 -f examples/workspace/Dockerfile -t "$REPO:$TAG" --push .
gcloud artifacts docker images describe "$REPO:$TAG" --format 'value(image_summary.fully_qualified_digest)'
```

The last line prints the image by digest. On an Apple silicon Mac,
building for `linux/amd64` runs under emulation and takes a while; an Arm
`machine_type` (`c4a-standard-1`, or `t2a-standard-1` where it's offered)
boots COS for arm64 instead, and you build with `--platform linux/arm64`,
natively. The module picks Hyperdisk for C4A and N4A, which take nothing
else.

### 5. Second apply: the VM

Set `image` in `terraform.tfvars` to that digest, then
`terraform apply`. On its first boot the VM formats the data disk, logs
Tailscale in with the key, and starts the hub. In a minute or two:

- Tailscale's **Machines** page shows `wheelhouse-workspace`, tagged.
- From a member's Mac:
  `curl https://wheelhouse-workspace.TAILNET.ts.net/team/v1/hello` answers
  `{"workspace":false}`.

### 6. Make the workspace, now

Until it's made, the setup code is on the disk and in the VM's logs.
Read it one of two ways:

```sh
eval "$(terraform output -raw setup_code_command)"      # through IAP (ssh_members)
eval "$(terraform output -raw setup_code_from_logs)"    # from Cloud Logging
```

Then, on the first admin's Mac, as [teams-start.md](teams-start.md#2-make-the-workspace-the-first-admin)
says:

```sh
wh team create Acme --prefix ACME --url https://wheelhouse-workspace.TAILNET.ts.net --setup whs1.…
wh team members                 # you, as its only admin
```

If `wh team create` says the hub has a workspace already, someone made it
first: don't use it. Wipe and start again (see [Retiring it](#retiring-it)).

Last, destroy the auth key's version; it's spent, and the VM doesn't
need it again:

```sh
S=$(terraform output -raw tailscale_auth_key_secret)    # projects/P/secrets/NAME
for v in $(gcloud secrets versions list "$S" --filter state=enabled --format 'value(name.basename())'); do
  gcloud secrets versions destroy "$v" --secret "$S" --quiet
done
```

On later boots the VM finds Tailscale logged in and goes on without it.

Then invite the team: [teams-start.md](teams-start.md#3-invite-each-engineer).

## People joining and leaving

- **Joining:** a tailnet admin adds their login to `group:wheelhouse`;
  a workspace admin runs `wh team invite HANDLE` and hands them the code;
  they run `wh team join` on their Mac.
- **Leaving:** a workspace admin runs `wh team remove HANDLE` first. That
  revokes every key they had, on every hub, from then on; what they signed
  before still verifies. Then take them out of `group:wheelhouse`, which
  closes the door too.
- **A lost or stolen Mac:** both of the above, at once, and remove the
  device in Tailscale. The person rejoins from a new Mac with a new
  invite, and new keys.
- **Operators leaving:** take them out of the ops group. Nothing on the
  VM is theirs.

## Upgrading

- **Wheelhouse:** build and push the new commit (step 4), set `image` to
  its digest, and `terraform apply`. The plan replaces the VM, since its
  boot config changed, and keeps the data disk and the tailnet name. The
  hub is gone for a minute or two; members' hubs keep their writes in
  their outboxes and send them when it's back. Build it from the commit
  the team's Macs run.
- **Tailscale:** set `tailscale_image` to a newer digest, and apply.
- **COS:** it updates itself; an update takes effect at the next reboot.
  `gcloud compute instances reset wheelhouse-workspace --zone ZONE` at a
  quiet moment.
- **Before any of these,** take a snapshot:
  `gcloud compute disks snapshot wheelhouse-workspace-data --zone ZONE --snapshot-names wheelhouse-pre-upgrade-$(date +%Y%m%d)`.

## Backups and restoring

The data disk is snapshotted daily and kept for `snapshot_retention_days`
(14), in the region. The hub keeps its state in SQLite with a
write-ahead log, which recovers from a snapshot taken mid-write.

**A restore goes back in time, and members' hubs notice.** A member whose
hub saw entries newer than the snapshot holds a chain the restored hub
can't extend: when the hub orders a new entry where that member already
has one, their hub stops following that docket and says so ("Not
followed"). That's the record doing its job, and there's no way yet to
put those entries back on a restored hub. So snapshots are for losing the
disk, not for undoing a mistake.

To restore:

```sh
terraform state rm module.workspace.google_compute_disk.data    # the old disk stays, untracked
# in terraform.tfvars:
#   data_disk_name     = "wheelhouse-workspace-data-r1"
#   data_disk_snapshot = "projects/PROJECT/global/snapshots/SNAPSHOT"
terraform apply                                                   # a new disk, and a new VM on it
```

Delete the old disk once you're sure you don't need it.

## Day to day

- **Is it up?** `wh team` on any member's Mac says connected or not.
  The VM has no address a Google uptime check can reach, and the module
  adds no alerting: if you want it, a log-based alert on
  `wh-workspace.service` failing is the place to start.
- **Logs:** in Cloud Logging under the VM, or over SSH:
  `sudo journalctl -u wh-workspace -u wh-tailscale -u wh-data`.
- **The boot:** each boot, the VM checks and mounts the data disk
  (`wh-data`), reads the auth key (`wh-tailscale`), pulls the image by
  digest and starts the hub (`wh-workspace`). If any step fails, the hub
  and Tailscale try again every 10 seconds, the disk check with them,
  and the journal says why. A disk is formatted only when it's new:
  nothing on it and zeros at its start. One that can't be read, or whose
  filesystem is damaged, is left alone for you to repair.
- **Cost:** in `us-central1` at list prices, an `e2-small`, its disks,
  Cloud NAT and the snapshots come to roughly $20 a month.

## Retiring it

The data disk outlives `terraform destroy` on purpose. To remove
everything:

```sh
terraform state rm module.workspace.google_compute_disk.data
terraform destroy
gcloud compute disks delete wheelhouse-workspace-data --zone ZONE
gcloud compute snapshots list --filter 'labels.app=wheelhouse-workspace'   # then delete them
```

Then remove the machine in Tailscale. Members' hubs keep their copies of
every shared docket, and `wh verify` still checks them.

## What this protects, and what it doesn't

| Who | Can | Can't |
| --- | --- | --- |
| Anyone on the internet | Nothing | Reach the VM: no public IP, no ingress, Funnel off |
| Anything else in the project's network | Nothing | Reach the VM (no ingress), or the hub's port (loopback only) |
| Someone on the tailnet, not in the group | Nothing | Reach the node: the policy grants only `group:wheelhouse` |
| A member | What [teams.md](teams.md#threat-model) says a member can | Reach anything on the VM but the hub, through Serve |
| An operator | Read what's shared, stop the hub, run another image, read the setup code | Forge an entry, add a member, or rewrite what members' hubs have without them noticing |
| A project log reader, before the workspace is made | Read the setup code, and make the workspace if they're also in the group | Anything once it's made: the code is gone |
| A tailnet admin | Add themselves to the group, put a node on the tag | Get past the member key: every request and entry is signed |

What it doesn't do yet:

- **The setup code reaches the logs.** `wh serve --workspace` prints it,
  and COS sends the containers' output to Cloud Logging
  (`cloud_logging = false` stops that, and the rest of the logs with it). Make the workspace right after the
  first boot, and check `wh team members` says it's yours.
- **What's shared isn't encrypted against the operators.** It's on the
  disk in the clear, encrypted at rest by Google or your key. Choose
  operators as you'd choose admins.
- **No alerting** (above), and **no high availability**: one VM, one
  zone. Members' outboxes ride out an outage; a lost zone means a restore.
