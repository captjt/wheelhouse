# A VPC of its own. Nothing comes in: members reach the hub over Tailscale,
# which the VM dials out to. Going out is through Cloud NAT.

resource "google_compute_network" "hub" {
  project                 = var.project_id
  name                    = var.name
  auto_create_subnetworks = false
  routing_mode            = "REGIONAL"

  depends_on = [google_project_service.this]
}

resource "google_compute_subnetwork" "hub" {
  project                  = var.project_id
  region                   = var.region
  name                     = var.name
  network                  = google_compute_network.hub.id
  ip_cidr_range            = var.subnet_cidr
  private_ip_google_access = true

  log_config {
    aggregation_interval = "INTERVAL_5_MIN"
    flow_sampling        = 0.5
    metadata             = "INCLUDE_ALL_METADATA"
  }
}

resource "google_compute_router" "hub" {
  project = var.project_id
  region  = var.region
  name    = var.name
  network = google_compute_network.hub.id
}

resource "google_compute_router_nat" "hub" {
  project                            = var.project_id
  region                             = var.region
  name                               = var.name
  router                             = google_compute_router.hub.name
  nat_ip_allocate_option             = "AUTO_ONLY"
  source_subnetwork_ip_ranges_to_nat = "LIST_OF_SUBNETWORKS"

  subnetwork {
    name                    = google_compute_subnetwork.hub.id
    source_ip_ranges_to_nat = ["ALL_IP_RANGES"]
  }

  # Lets Tailscale find direct paths to members more often, rather than
  # relaying through DERP. Either way the traffic is end-to-end encrypted.
  enable_endpoint_independent_mapping = true

  log_config {
    enable = true
    filter = "ERRORS_ONLY"
  }
}

# --- Ingress: none ------------------------------------------------------------

resource "google_compute_firewall" "deny_ingress" {
  project     = var.project_id
  name        = "${var.name}-deny-ingress"
  network     = google_compute_network.hub.id
  direction   = "INGRESS"
  priority    = 65534
  description = "Nothing reaches the workspace hub's VM from the network. Members come in over Tailscale, which dials out."

  deny {
    protocol = "all"
  }
  source_ranges = ["0.0.0.0/0"]

  log_config {
    metadata = "INCLUDE_ALL_METADATA"
  }
}

# IAP's TCP forwarding range, for SSH only, only when someone may SSH.
resource "google_compute_firewall" "iap_ssh" {
  count       = length(var.ssh_members) > 0 ? 1 : 0
  project     = var.project_id
  name        = "${var.name}-iap-ssh"
  network     = google_compute_network.hub.id
  direction   = "INGRESS"
  priority    = 1000
  description = "SSH through IAP, for the members of var.ssh_members."

  allow {
    protocol = "tcp"
    ports    = ["22"]
  }
  source_ranges = ["35.235.240.0/20"]
  target_tags   = [local.network_tag]

  log_config {
    metadata = "INCLUDE_ALL_METADATA"
  }
}

# --- Egress: what Tailscale and Google's APIs need --------------------------

resource "google_compute_firewall" "allow_egress" {
  count              = var.restrict_egress ? 1 : 0
  project            = var.project_id
  name               = "${var.name}-allow-egress"
  network            = google_compute_network.hub.id
  direction          = "EGRESS"
  priority           = 1000
  description        = "HTTPS (Tailscale's control plane and DERP, Google's APIs, image registries, COS updates) and UDP (Tailscale's WireGuard and STUN)."
  destination_ranges = ["0.0.0.0/0"]
  target_tags        = [local.network_tag]

  allow {
    protocol = "tcp"
    ports    = ["443"]
  }
  allow {
    protocol = "udp"
  }
}

resource "google_compute_firewall" "deny_egress" {
  count              = var.restrict_egress ? 1 : 0
  project            = var.project_id
  name               = "${var.name}-deny-egress"
  network            = google_compute_network.hub.id
  direction          = "EGRESS"
  priority           = 65534
  description        = "Everything else outbound is refused, and logged."
  destination_ranges = ["0.0.0.0/0"]

  deny {
    protocol = "all"
  }

  log_config {
    metadata = "INCLUDE_ALL_METADATA"
  }
}
