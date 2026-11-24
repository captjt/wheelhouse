variable "project_id" {
  description = "The GCP project the workspace hub runs in. Give it a project of its own if you can."
  type        = string
}

variable "region" {
  description = "Where everything lives: the subnet, the image repository, the auth key's replica and the snapshots."
  type        = string
}

variable "zone" {
  description = "The zone of the VM and its data disk. Must be in var.region."
  type        = string

  validation {
    condition     = startswith(var.zone, "${var.region}-")
    error_message = "zone must be in region."
  }
}

variable "name" {
  description = "The VM's name, and the prefix of everything else this module makes."
  type        = string
  default     = "wheelhouse-workspace"

  validation {
    # It's also the service account's id, which is at most 30 characters.
    condition     = can(regex("^[a-z][a-z0-9-]{4,22}[a-z0-9]$", var.name))
    error_message = "name is 6 to 24 characters: lowercase letters, digits and hyphens, starting with a letter."
  }
}

variable "image" {
  description = <<-EOT
    The workspace hub's image, by digest:
    REGION-docker.pkg.dev/PROJECT/REPO/wheelhouse-workspace@sha256:…
    Leave it null on the first apply: that makes the repository to push it to,
    and the secret for the Tailscale auth key, but no VM yet.
  EOT
  type        = string
  default     = null

  validation {
    condition     = var.image == null || can(regex("^[a-z0-9.-]+(:[0-9]+)?/[a-z0-9._/-]+(:[A-Za-z0-9._-]+)?@sha256:[0-9a-f]{64}$", var.image))
    error_message = "image must name a digest (…@sha256:<64 hex>), so what runs is exactly what you pushed."
  }
}

variable "tailscale_image" {
  description = "Tailscale's container image, by digest. It publishes the hub on the tailnet with Tailscale Serve."
  type        = string
  default     = "docker.io/tailscale/tailscale:v1.104.7@sha256:72c1e19e0aedca96b8533bbb0b4368564410ff20b17e53b804eb6c1f04a31be4"

  validation {
    condition     = can(regex("@sha256:[0-9a-f]{64}$", var.tailscale_image))
    error_message = "tailscale_image must name a digest (…@sha256:<64 hex>)."
  }
}

variable "tailscale_hostname" {
  description = "The machine's name on the tailnet: the hub is https://HOSTNAME.YOUR-TAILNET.ts.net. Defaults to var.name."
  type        = string
  default     = null

  validation {
    condition     = var.tailscale_hostname == null || can(regex("^[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?$", var.tailscale_hostname))
    error_message = "tailscale_hostname is a DNS label: lowercase letters, digits and hyphens."
  }
}

variable "tailscale_tags" {
  description = "Tags the machine advertises. The auth key must be made for exactly these. The machine is tagged, so it belongs to no person and its key doesn't expire."
  type        = list(string)
  default     = ["tag:wheelhouse-workspace"]

  validation {
    condition     = length(var.tailscale_tags) > 0 && alltrue([for t in var.tailscale_tags : can(regex("^tag:[A-Za-z0-9-]+$", t))])
    error_message = "tailscale_tags is one or more tags like tag:wheelhouse-workspace."
  }
}

variable "machine_type" {
  description = "The VM's machine type. The workspace hub is small: it orders entries and keeps them in SQLite. An Arm type (t2a-, c4a-, n4a-) boots COS for arm64 and needs an arm64 image, which an Apple silicon Mac builds natively."
  type        = string
  default     = "e2-small"
}

variable "data_disk_size_gb" {
  description = "The data disk's size. It holds the member log, every shared docket and the files they name (at most 25 MB each)."
  type        = number
  default     = 20

  validation {
    condition     = var.data_disk_size_gb >= 10
    error_message = "data_disk_size_gb is at least 10."
  }
}

variable "data_disk_type" {
  description = "The data disk's type. Defaults to pd-balanced, or hyperdisk-balanced on machine types that take only Hyperdisk (c4a-, n4a-)."
  type        = string
  default     = null
}

variable "data_disk_name" {
  description = "The data disk's name. Defaults to NAME-data. Change it only to restore from a snapshot (docs/teams-gcp.md)."
  type        = string
  default     = null
}

variable "data_disk_snapshot" {
  description = "A snapshot to make the data disk from, when restoring. Null makes a blank disk, formatted at first boot."
  type        = string
  default     = null
}

variable "snapshot_retention_days" {
  description = "How long the daily snapshots of the data disk are kept."
  type        = number
  default     = 14

  validation {
    condition     = var.snapshot_retention_days >= 1
    error_message = "snapshot_retention_days is at least 1."
  }
}

variable "snapshot_start_time" {
  description = "When the daily snapshot starts, in UTC (HH:00)."
  type        = string
  default     = "04:00"

  validation {
    condition     = can(regex("^([01][0-9]|2[0-3]):00$", var.snapshot_start_time))
    error_message = "snapshot_start_time is a whole hour in UTC, like 04:00."
  }
}

variable "kms_key" {
  description = <<-EOT
    A Cloud KMS key to encrypt the boot and data disks with (CMEK), as
    projects/P/locations/REGION/keyRings/R/cryptoKeys/K. Null uses Google's
    own keys. The module grants the project's Compute Engine service agent
    use of the key, so whoever applies needs to be able to set its IAM.
  EOT
  type        = string
  default     = null
}

variable "subnet_cidr" {
  description = "The workspace hub's subnet, in a VPC of its own. Nothing else needs to reach it: members come in over Tailscale."
  type        = string
  default     = "10.77.0.0/28"
}

variable "restrict_egress" {
  description = "Allow the VM out only on TCP 443 (Tailscale's control plane and relays, Google's APIs, the image registries) and UDP (Tailscale's direct paths); everything else outbound is refused and logged."
  type        = bool
  default     = true
}

variable "ssh_members" {
  description = <<-EOT
    Who may SSH to the VM, through IAP with OS Login, as admins: to read the
    setup code, logs, or fix a stuck boot. Like "group:wheelhouse-ops@acme.com".
    Empty means no SSH at all: no firewall rule for it, and no one granted.
  EOT
  type        = list(string)
  default     = []
}

variable "require_2fa_for_ssh" {
  description = "OS Login asks for a second factor on SSH. Needs 2-step verification on the members' Google accounts."
  type        = bool
  default     = false
}

variable "image_publishers" {
  description = "Who may push images to the repository, like \"group:wheelhouse-ops@acme.com\". Tags there can't be moved once pushed."
  type        = list(string)
  default     = []
}

variable "cloud_logging" {
  description = "Send the VM's logs (the hub's, Tailscale's, the system's) to Cloud Logging. Until the workspace is made, they hold its setup code: see docs/teams-gcp.md."
  type        = bool
  default     = true
}

variable "labels" {
  description = "Labels for everything this module makes."
  type        = map(string)
  default     = {}
}
