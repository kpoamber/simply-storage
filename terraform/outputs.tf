output "server_ip" {
  description = "Public IPv4 address of the server"
  value       = hcloud_server.innovare.ipv4_address
}

output "server_status" {
  description = "Current status of the server"
  value       = hcloud_server.innovare.status
}

output "ssh_connection_string" {
  description = "SSH command to connect to the server"
  value       = "ssh deploy@${hcloud_server.innovare.ipv4_address}"
}

output "backup_volume_id" {
  description = "ID of the backup volume"
  value       = hcloud_volume.backups.id
}
