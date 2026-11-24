locals {
  repository = "${var.region}-docker.pkg.dev/${var.project_id}/${google_artifact_registry_repository.images.repository_id}"
  ssh        = "gcloud compute ssh ${var.name} --project ${var.project_id} --zone ${var.zone} --tunnel-through-iap"
}

output "image_repository" {
  description = "Push the workspace hub's image here, then set var.image to it by digest."
  value       = "${local.repository}/wheelhouse-workspace"
}

output "tailscale_auth_key_secret" {
  description = "The secret the VM reads its Tailscale auth key from."
  value       = google_secret_manager_secret.tailscale_auth_key.id
}

output "add_tailscale_auth_key" {
  description = "Adds the auth key from $TS_AUTHKEY as the secret's new version, without it touching Terraform."
  value       = "printf %s \"$TS_AUTHKEY\" | gcloud secrets versions add ${google_secret_manager_secret.tailscale_auth_key.secret_id} --project ${var.project_id} --data-file=-"
}

output "service_account" {
  description = "The VM's service account."
  value       = google_service_account.vm.email
}

output "instance" {
  description = "The VM, once var.image is set."
  value       = local.deploy ? google_compute_instance.hub[0].self_link : null
}

output "data_disk" {
  description = "The disk that holds the workspace."
  value       = google_compute_disk.data.self_link
}

output "workspace_url" {
  description = "The address members use, once Tailscale is up. Your tailnet's name is in the Tailscale admin console under DNS."
  value       = "https://${local.hostname}.<your-tailnet>.ts.net"
}

output "ssh_command" {
  description = "SSH to the VM through IAP (members of var.ssh_members)."
  value       = length(var.ssh_members) > 0 ? local.ssh : null
}

output "setup_code_command" {
  description = "Prints the one-time setup code for wh team create, until the workspace is made."
  value       = length(var.ssh_members) > 0 ? "${local.ssh} --command 'sudo cat /mnt/disks/wh/workspace/setup-code'" : null
}

output "setup_code_from_logs" {
  description = "Finds the setup code in Cloud Logging instead (needs roles/logging.viewer)."
  value       = var.cloud_logging && local.deploy ? "gcloud logging read --project ${var.project_id} --freshness 7d 'resource.type=\"gce_instance\" AND resource.labels.instance_id=\"${google_compute_instance.hub[0].instance_id}\" AND \"whs1.\"'" : null
}
