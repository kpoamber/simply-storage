# --- SSH Key ---

resource "hcloud_ssh_key" "deploy" {
  name       = var.ssh_key_name
  public_key = var.ssh_public_key
}

# --- Firewall ---

resource "hcloud_firewall" "innovare" {
  name = "innovare-storage-fw"

  rule {
    description = "SSH"
    direction   = "in"
    protocol    = "tcp"
    port        = "22"
    source_ips  = ["0.0.0.0/0", "::/0"]
  }

  rule {
    description = "HTTP"
    direction   = "in"
    protocol    = "tcp"
    port        = "80"
    source_ips  = ["0.0.0.0/0", "::/0"]
  }

  rule {
    description = "HTTPS"
    direction   = "in"
    protocol    = "tcp"
    port        = "443"
    source_ips  = ["0.0.0.0/0", "::/0"]
  }
}

# --- Network (for potential multi-node setups) ---

resource "hcloud_network" "innovare" {
  name     = "innovare-network"
  ip_range = "10.0.0.0/16"
}

resource "hcloud_network_subnet" "innovare" {
  network_id   = hcloud_network.innovare.id
  type         = "cloud"
  network_zone = "eu-central"
  ip_range     = "10.0.1.0/24"
}

# --- Backup Volume ---

resource "hcloud_volume" "backups" {
  name      = "innovare-backups"
  size      = var.backup_volume_size
  location  = var.location
  format    = "ext4"
  automount = false
}

resource "hcloud_volume_attachment" "backups" {
  volume_id = hcloud_volume.backups.id
  server_id = hcloud_server.innovare.id
  automount = false
}

# --- Server ---

resource "hcloud_server" "innovare" {
  name        = "innovare-storage"
  server_type = var.server_type
  image       = var.image
  location    = var.location

  ssh_keys = [hcloud_ssh_key.deploy.id]

  firewall_ids = [hcloud_firewall.innovare.id]

  user_data = templatefile("${path.module}/cloud-init.yml", {
    deploy_profile     = var.deploy_profile
    domain             = var.domain
    ghcr_user          = var.ghcr_user
    ghcr_token         = var.ghcr_token
    app_env_vars       = var.app_env_vars
    volume_device      = hcloud_volume.backups.linux_device
    ssh_authorized_key = var.ssh_public_key
  })

  network {
    network_id = hcloud_network.innovare.id
    ip         = "10.0.1.2"
  }

  depends_on = [
    hcloud_network_subnet.innovare,
  ]

  labels = {
    app     = "innovare-storage"
    profile = var.deploy_profile
  }
}
