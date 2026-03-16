variable "hcloud_token" {
  description = "Hetzner Cloud API token"
  type        = string
  sensitive   = true
}

variable "server_type" {
  description = "Hetzner server type (e.g. cx22, cx32, cx42)"
  type        = string
  default     = "cx22"
}

variable "location" {
  description = "Hetzner datacenter location"
  type        = string
  default     = "nbg1"
}

variable "ssh_key_name" {
  description = "Name for the SSH key resource in Hetzner"
  type        = string
  default     = "innovare-deploy"
}

variable "ssh_public_key" {
  description = "SSH public key content for server access"
  type        = string
}

variable "domain" {
  description = "Domain name for the application"
  type        = string
  default     = "storage.example.com"
}

variable "deploy_profile" {
  description = "Deployment profile: small, medium, or large"
  type        = string
  default     = "small"

  validation {
    condition     = contains(["small", "medium", "large"], var.deploy_profile)
    error_message = "deploy_profile must be one of: small, medium, large"
  }
}

variable "app_env_vars" {
  description = "Application environment variables to pass to the server"
  type        = map(string)
  default     = {}
  sensitive   = true
}

variable "backup_volume_size" {
  description = "Size in GB for the backup volume"
  type        = number
  default     = 50
}

variable "image" {
  description = "OS image for the server"
  type        = string
  default     = "ubuntu-24.04"
}

variable "ghcr_user" {
  description = "GitHub Container Registry username"
  type        = string
  default     = ""
}

variable "ghcr_token" {
  description = "GitHub Container Registry token (read:packages)"
  type        = string
  default     = ""
  sensitive   = true
}
