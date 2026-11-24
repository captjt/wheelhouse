# A workspace hub on GCP

Terraform for a Wheelhouse workspace hub (`wh serve --workspace`) in a GCP
project: one private VM on Container-Optimized OS, the hub on its loopback,
Tailscale Serve beside it as the only way in, and the workspace's state on
a separate, snapshotted disk.

- `module/`: the module. `variables.tf` says what each input does.
- `example/`: a root config that uses it, with `terraform.tfvars.example`
  and `backend.tf.example`.
- `tailscale-policy.hujson`: what the tailnet policy needs.

[docs/teams-gcp.md](../../../docs/teams-gcp.md) is how a team deploys and
runs it: who holds which role, the two applies, the auth key, the setup
code, people joining and leaving, upgrades, restores, and what it protects.

`just check-gcp` formats, validates and tests it (against a mocked
provider: no project or credentials needed).
