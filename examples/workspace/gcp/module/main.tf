# A Wheelhouse workspace hub (wh serve --workspace) on one private VM.
#
# The hub trusts the Tailscale-User-Login header that Tailscale Serve sets,
# so nothing but Serve may ever reach its port. Everything here follows
# from that: no public IP, no ingress at all, the hub bound to 127.0.0.2
# on the VM (cloud-init.yaml.tftpl says why not 127.0.0.1), and Serve
# running beside it on the same VM. docs/teams-gcp.md
# has the why of each piece and how to run it as a team.

locals {
  hostname       = coalesce(var.tailscale_hostname, var.name)
  data_disk_name = coalesce(var.data_disk_name, "${var.name}-data")
  network_tag    = var.name
  deploy         = var.image != null
  registry_host  = local.deploy ? split("/", var.image)[0] : ""
  labels         = merge({ app = "wheelhouse-workspace" }, var.labels)
  # Arm machine types boot COS's arm64 build, and run an arm64 image.
  arm64 = can(regex("^(t2a|c4a|n4a)-", var.machine_type))
  # C4A and N4A take only Hyperdisk; T2A can't live-migrate.
  hyperdisk      = can(regex("^(c4a|n4a)-", var.machine_type))
  disk_type      = local.hyperdisk ? "hyperdisk-balanced" : "pd-balanced"
  data_disk_type = coalesce(var.data_disk_type, local.disk_type)
  live_migrate   = !can(regex("^t2a-", var.machine_type))

  services = toset([
    "artifactregistry.googleapis.com",
    "compute.googleapis.com",
    "iap.googleapis.com",
    "logging.googleapis.com",
    "monitoring.googleapis.com",
    "secretmanager.googleapis.com",
  ])

  cloud_init = local.deploy ? templatefile("${path.module}/cloud-init.yaml.tftpl", {
    project_id      = var.project_id
    secret_id       = google_secret_manager_secret.tailscale_auth_key.secret_id
    image           = var.image
    tailscale_image = var.tailscale_image
    registry_host   = local.registry_host
    hostname        = local.hostname
    tags            = join(",", var.tailscale_tags)
    data_device     = "wh-data"
  }) : ""
}

data "google_project" "this" {
  project_id = var.project_id
}

resource "google_project_service" "this" {
  for_each           = local.services
  project            = var.project_id
  service            = each.value
  disable_on_destroy = false
}

# --- The image ---------------------------------------------------------------

resource "google_artifact_registry_repository" "images" {
  project       = var.project_id
  location      = var.region
  repository_id = var.name
  description   = "Wheelhouse workspace hub images"
  format        = "DOCKER"
  labels        = local.labels

  docker_config {
    immutable_tags = true
  }

  depends_on = [google_project_service.this]
}

resource "google_artifact_registry_repository_iam_member" "vm_reads" {
  project    = var.project_id
  location   = google_artifact_registry_repository.images.location
  repository = google_artifact_registry_repository.images.name
  role       = "roles/artifactregistry.reader"
  member     = google_service_account.vm.member
}

resource "google_artifact_registry_repository_iam_member" "publishers" {
  for_each   = toset(var.image_publishers)
  project    = var.project_id
  location   = google_artifact_registry_repository.images.location
  repository = google_artifact_registry_repository.images.name
  role       = "roles/artifactregistry.writer"
  member     = each.value
}

# --- The Tailscale auth key --------------------------------------------------
#
# Terraform makes the secret, never its value: you add the key with gcloud,
# so it's never in Terraform's state or plan.

resource "google_secret_manager_secret" "tailscale_auth_key" {
  project   = var.project_id
  secret_id = "${var.name}-tailscale-auth-key"
  labels    = local.labels

  replication {
    user_managed {
      replicas {
        location = var.region
      }
    }
  }

  depends_on = [google_project_service.this]
}

resource "google_secret_manager_secret_iam_member" "vm_reads_key" {
  project   = var.project_id
  secret_id = google_secret_manager_secret.tailscale_auth_key.secret_id
  role      = "roles/secretmanager.secretAccessor"
  member    = google_service_account.vm.member
}

# --- The VM's identity -------------------------------------------------------
#
# Its own service account, with nothing at the project level but writing
# logs and metrics. It reads one repository and one secret.

resource "google_service_account" "vm" {
  project      = var.project_id
  account_id   = var.name
  display_name = "Wheelhouse workspace hub VM"
  description  = "Runs the Wheelhouse workspace hub. Reads its image and its Tailscale auth key; writes logs and metrics."
}

resource "google_project_iam_member" "vm" {
  for_each = toset(["roles/logging.logWriter", "roles/monitoring.metricWriter"])
  project  = var.project_id
  role     = each.value
  member   = google_service_account.vm.member
}

# --- Disks -------------------------------------------------------------------

# The workspace's state: the member log, every shared docket, the files they
# name, the setup code until it's used, and Tailscale's node key. Terraform
# refuses to destroy it; docs/teams-gcp.md says how to retire it on purpose.
resource "google_compute_disk" "data" {
  project  = var.project_id
  zone     = var.zone
  name     = local.data_disk_name
  type     = local.data_disk_type
  size     = var.data_disk_size_gb
  snapshot = var.data_disk_snapshot
  labels   = local.labels

  dynamic "disk_encryption_key" {
    for_each = var.kms_key == null ? [] : [var.kms_key]
    content {
      kms_key_self_link = disk_encryption_key.value
    }
  }

  lifecycle {
    prevent_destroy = true
  }

  depends_on = [google_project_service.this, google_kms_crypto_key_iam_member.compute]
}

resource "google_compute_resource_policy" "snapshots" {
  project = var.project_id
  region  = var.region
  name    = "${var.name}-daily"

  snapshot_schedule_policy {
    schedule {
      daily_schedule {
        days_in_cycle = 1
        start_time    = var.snapshot_start_time
      }
    }
    retention_policy {
      max_retention_days    = var.snapshot_retention_days
      on_source_disk_delete = "KEEP_AUTO_SNAPSHOTS"
    }
    snapshot_properties {
      labels            = local.labels
      storage_locations = [var.region]
    }
  }

  depends_on = [google_project_service.this]
}

resource "google_compute_disk_resource_policy_attachment" "snapshots" {
  project = var.project_id
  zone    = var.zone
  disk    = google_compute_disk.data.name
  name    = google_compute_resource_policy.snapshots.name
}

resource "google_kms_crypto_key_iam_member" "compute" {
  count         = var.kms_key == null ? 0 : 1
  crypto_key_id = var.kms_key
  role          = "roles/cloudkms.cryptoKeyEncrypterDecrypter"
  member        = "serviceAccount:service-${data.google_project.this.number}@compute-system.iam.gserviceaccount.com"

  # The service agent exists once the Compute Engine API is on.
  depends_on = [google_project_service.this]
}

# --- The VM ------------------------------------------------------------------

data "google_compute_image" "cos" {
  project = "cos-cloud"
  family  = local.arm64 ? "cos-arm64-stable" : "cos-stable"
}

# A new boot config replaces the VM (its state is on the data disk), so
# what runs is always what's in the plan.
resource "terraform_data" "boot_config" {
  count = local.deploy ? 1 : 0
  input = sha256(local.cloud_init)
}

resource "google_compute_instance" "hub" {
  count        = local.deploy ? 1 : 0
  project      = var.project_id
  zone         = var.zone
  name         = var.name
  machine_type = var.machine_type
  tags         = [local.network_tag]
  labels       = local.labels

  boot_disk {
    auto_delete       = true
    kms_key_self_link = var.kms_key
    initialize_params {
      image  = data.google_compute_image.cos.self_link
      size   = 20
      type   = local.disk_type
      labels = local.labels
    }
  }

  attached_disk {
    source      = google_compute_disk.data.id
    device_name = "wh-data"
    mode        = "READ_WRITE"
  }

  # No access_config: no public IP. Out through Cloud NAT; in only over
  # Tailscale, which dials out.
  network_interface {
    subnetwork = google_compute_subnetwork.hub.id
  }

  service_account {
    email  = google_service_account.vm.email
    scopes = ["cloud-platform"]
  }

  shielded_instance_config {
    enable_secure_boot          = true
    enable_vtpm                 = true
    enable_integrity_monitoring = true
  }

  metadata = {
    user-data                 = local.cloud_init
    enable-oslogin            = "TRUE"
    enable-oslogin-2fa        = var.require_2fa_for_ssh ? "TRUE" : "FALSE"
    block-project-ssh-keys    = "TRUE"
    serial-port-enable        = "FALSE"
    google-logging-enabled    = var.cloud_logging ? "true" : "false"
    google-monitoring-enabled = "true"
    cos-update-strategy       = "update_enabled"
  }

  scheduling {
    automatic_restart   = true
    on_host_maintenance = local.live_migrate ? "MIGRATE" : "TERMINATE"
  }

  allow_stopping_for_update = true

  lifecycle {
    # COS updates itself in place; a newer image in the family isn't a
    # reason to replace the VM.
    ignore_changes = [boot_disk[0].initialize_params[0].image]
    # A new data disk (a restore) replaces the VM too: never swapped under a running hub.
    replace_triggered_by = [terraform_data.boot_config[0], google_compute_disk.data.id]
  }

  depends_on = [
    google_project_iam_member.vm,
    google_artifact_registry_repository_iam_member.vm_reads,
    google_secret_manager_secret_iam_member.vm_reads_key,
    google_compute_router_nat.hub,
    google_kms_crypto_key_iam_member.compute,
  ]
}

# --- Who may SSH -------------------------------------------------------------
#
# Through IAP only, with OS Login. Granted on this one VM, its service
# account and IAP tunnel, plus compute.viewer so gcloud can find the VM.

resource "google_iap_tunnel_instance_iam_member" "ssh" {
  for_each = local.deploy ? toset(var.ssh_members) : toset([])
  project  = var.project_id
  zone     = var.zone
  instance = google_compute_instance.hub[0].name
  role     = "roles/iap.tunnelResourceAccessor"
  member   = each.value
}

resource "google_compute_instance_iam_member" "ssh" {
  for_each      = local.deploy ? toset(var.ssh_members) : toset([])
  project       = var.project_id
  zone          = var.zone
  instance_name = google_compute_instance.hub[0].name
  role          = "roles/compute.osAdminLogin"
  member        = each.value
}

resource "google_service_account_iam_member" "ssh" {
  for_each           = toset(var.ssh_members)
  service_account_id = google_service_account.vm.name
  role               = "roles/iam.serviceAccountUser"
  member             = each.value
}

resource "google_project_iam_member" "ssh_viewer" {
  for_each = toset(var.ssh_members)
  project  = var.project_id
  role     = "roles/compute.viewer"
  member   = each.value
}
