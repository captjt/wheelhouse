# An example root config: one workspace hub in its own project.
# docs/teams-gcp.md walks through it. Copy this folder, fill in
# terraform.tfvars (from terraform.tfvars.example), and keep the state in
# a bucket the team's operators share (backend.tf.example).

terraform {
  required_version = ">= 1.7"

  required_providers {
    google = {
      source  = "hashicorp/google"
      version = ">= 7.0, < 9.0"
    }
  }
}

provider "google" {
  project = var.project_id
  region  = var.region

  default_labels = {
    managed-by = "terraform"
  }
}

module "workspace" {
  source = "../module"

  project_id = var.project_id
  region     = var.region
  zone       = var.zone

  # Null on the first apply; then the digest you pushed.
  image = var.image

  ssh_members      = var.ssh_members
  image_publishers = var.image_publishers
  kms_key          = var.kms_key
}

variable "project_id" {
  type = string
}

variable "region" {
  type    = string
  default = "us-central1"
}

variable "zone" {
  type    = string
  default = "us-central1-a"
}

variable "image" {
  type    = string
  default = null
}

variable "ssh_members" {
  type    = list(string)
  default = []
}

variable "image_publishers" {
  type    = list(string)
  default = []
}

variable "kms_key" {
  type    = string
  default = null
}

output "image_repository" {
  value = module.workspace.image_repository
}

output "add_tailscale_auth_key" {
  value = module.workspace.add_tailscale_auth_key
}

output "workspace_url" {
  value = module.workspace.workspace_url
}

output "ssh_command" {
  value = module.workspace.ssh_command
}

output "setup_code_command" {
  value = module.workspace.setup_code_command
}

output "setup_code_from_logs" {
  value = module.workspace.setup_code_from_logs
}
