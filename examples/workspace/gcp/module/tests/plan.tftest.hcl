# terraform test, with Google's provider mocked: no project or credentials.
# Checks both phases of a deploy and the guards on the inputs.

mock_provider "google" {
  mock_data "google_project" {
    defaults = { number = "123456789012" }
  }
  mock_data "google_compute_image" {
    defaults = { self_link = "https://www.googleapis.com/compute/v1/projects/cos-cloud/global/images/cos-stable-1" }
  }
}

variables {
  project_id = "acme-wh"
  region     = "us-central1"
  zone       = "us-central1-a"
}

run "first_apply_makes_no_vm" {
  command = plan

  assert {
    condition     = length(google_compute_instance.hub) == 0
    error_message = "no VM before there's an image"
  }
  assert {
    condition     = google_secret_manager_secret.tailscale_auth_key.secret_id == "wheelhouse-workspace-tailscale-auth-key"
    error_message = "the auth key's secret is made in the first apply"
  }
  assert {
    condition     = google_artifact_registry_repository.images.docker_config[0].immutable_tags
    error_message = "tags can't move once pushed"
  }
  assert {
    condition     = length(google_compute_firewall.iap_ssh) == 0
    error_message = "no SSH rule when no one may SSH"
  }
}

run "second_apply_makes_a_private_vm" {
  command = plan

  variables {
    image       = "us-central1-docker.pkg.dev/acme-wh/wheelhouse-workspace/wheelhouse-workspace@sha256:0000000000000000000000000000000000000000000000000000000000000000"
    ssh_members = ["group:ops@acme.com"]
  }

  assert {
    condition     = length(google_compute_instance.hub[0].network_interface[0].access_config) == 0
    error_message = "the VM has no public IP"
  }
  assert {
    condition     = strcontains(google_compute_instance.hub[0].metadata["user-data"], "-p 127.0.0.2:7700:7700") && strcontains(google_compute_instance.hub[0].metadata["user-data"], "\"Proxy\": \"http://127.0.0.2:7700\"") && !strcontains(google_compute_instance.hub[0].metadata["user-data"], "127.0.0.1:7700")
    error_message = "the hub is on 127.0.0.2 only, where Tailscale's netstack never forwards"
  }
  assert {
    condition     = strcontains(google_compute_instance.hub[0].metadata["user-data"], "wheelhouse-workspace@sha256:0000000000000000000000000000000000000000000000000000000000000000")
    error_message = "the VM runs the image by digest"
  }
  assert {
    condition     = strcontains(google_compute_instance.hub[0].metadata["user-data"], "{{.Config.User}}")
    error_message = "the hub runs as the image's own numeric user"
  }
  assert {
    condition     = strcontains(google_compute_instance.hub[0].metadata["user-data"], "configure-docker --registries=\"us-central1-docker.pkg.dev\"")
    error_message = "the VM can pull from Artifact Registry"
  }
  assert {
    condition     = google_compute_instance.hub[0].metadata["block-project-ssh-keys"] == "TRUE" && google_compute_instance.hub[0].metadata["enable-oslogin"] == "TRUE" && google_compute_instance.hub[0].metadata["serial-port-enable"] == "FALSE"
    error_message = "OS Login only, no project keys, no serial port"
  }
  assert {
    condition     = google_compute_instance.hub[0].shielded_instance_config[0].enable_secure_boot
    error_message = "Shielded VM with secure boot"
  }
  assert {
    condition     = google_compute_firewall.iap_ssh[0].source_ranges == toset(["35.235.240.0/20"])
    error_message = "SSH only from IAP"
  }
  assert {
    condition     = length(google_compute_firewall.deny_egress) == 1 && length(google_compute_firewall.allow_egress) == 1
    error_message = "egress is restricted by default"
  }
}

run "an_image_without_a_digest_is_refused" {
  command = plan
  variables {
    image = "us-central1-docker.pkg.dev/acme-wh/wheelhouse-workspace/wheelhouse-workspace:latest"
  }
  expect_failures = [var.image]
}

run "a_zone_outside_the_region_is_refused" {
  command = plan
  variables {
    zone = "europe-west4-a"
  }
  expect_failures = [var.zone]
}

run "a_bad_tag_is_refused" {
  command = plan
  variables {
    tailscale_tags = ["wheelhouse"]
  }
  expect_failures = [var.tailscale_tags]
}

run "an_arm_machine_boots_arm_cos" {
  command = plan
  variables {
    machine_type = "c4a-standard-1"
  }
  assert {
    condition     = data.google_compute_image.cos.family == "cos-arm64-stable"
    error_message = "an Arm machine type boots COS for arm64"
  }
  assert {
    condition     = google_compute_disk.data.type == "hyperdisk-balanced"
    error_message = "C4A takes only Hyperdisk"
  }
}

run "t2a_doesnt_live_migrate" {
  command = plan
  variables {
    machine_type = "t2a-standard-1"
    image        = "us-central1-docker.pkg.dev/acme-wh/wheelhouse-workspace/wheelhouse-workspace@sha256:0000000000000000000000000000000000000000000000000000000000000000"
  }
  assert {
    condition     = google_compute_instance.hub[0].scheduling[0].on_host_maintenance == "TERMINATE" && google_compute_disk.data.type == "pd-balanced"
    error_message = "T2A can't live-migrate, and takes Persistent Disk"
  }
}
