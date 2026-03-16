terraform {
  required_version = ">= 1.5.0"

  required_providers {
    hcloud = {
      source  = "hetznercloud/hcloud"
      version = "~> 1.45"
    }
  }

  # Local state by default. For team usage, switch to a remote backend:
  # backend "s3" {
  #   bucket   = "innovare-terraform-state"
  #   key      = "innovare-storage/terraform.tfstate"
  #   region   = "eu-central-1"
  #   encrypt  = true
  # }
}

provider "hcloud" {
  token = var.hcloud_token
}
