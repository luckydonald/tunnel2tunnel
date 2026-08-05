//! Best-effort guess of a human-friendly service name for a well-known port
//! number, used to auto-name a `port_configs` row that gets created on the
//! fly when an SSH client does `-R` on a port with no prior UI setup.

/// Return a short, human-friendly guess for what commonly runs on `port`, or
/// `None` if nothing well-known matches.
pub fn guess_service_name(port: i32) -> Option<&'static str> {
    match port {
        21 => Some("FTP"),
        22 => Some("SSH"),
        23 => Some("Telnet"),
        25 => Some("SMTP"),
        53 => Some("DNS"),
        80 => Some("HTTP"),
        110 => Some("POP3"),
        143 => Some("IMAP"),
        443 => Some("HTTPS"),
        445 => Some("SMB"),
        465 => Some("SMTPS"),
        587 => Some("SMTP (submission)"),
        993 => Some("IMAPS"),
        995 => Some("POP3S"),
        1433 => Some("MSSQL"),
        1521 => Some("Oracle DB"),
        2049 => Some("NFS"),
        3000 => Some("Node.js (dev)"),
        3001 => Some("Grafana"),
        3306 => Some("MySQL / MariaDB"),
        3389 => Some("RDP"),
        5000 => Some("Flask / dev"),
        5432 => Some("PostgreSQL"),
        5900 => Some("VNC"),
        6379 => Some("Redis"),
        6443 => Some("Kubernetes API"),
        7777 => Some("Game server"),
        8000 => Some("HTTP (dev)"),
        8006 => Some("Proxmox"),
        8080 => Some("HTTP (alt)"),
        8081 => Some("HTTP (alt 2)"),
        8096 => Some("Jellyfin"),
        8123 => Some("Home Assistant"),
        8384 => Some("Syncthing"),
        8443 => Some("HTTPS (alt)"),
        8880 => Some("phpMyAdmin"),
        8888 => Some("Jupyter"),
        9000 => Some("Portainer / MinIO"),
        9090 => Some("Prometheus"),
        9091 => Some("Transmission"),
        9100 => Some("Node exporter"),
        9200 => Some("Elasticsearch"),
        11211 => Some("Memcached"),
        25565 => Some("Minecraft"),
        27015 => Some("Steam / Source"),
        27017 => Some("MongoDB"),
        32400 => Some("Plex"),
        _ => None,
    }
}

/// Slugify a service name into a lowercase, hostname-safe string (e.g.
/// `"Home Assistant"` -> `"home_assistant"`, `"HTTP (alt)"` -> `"http_alt"`).
/// Used as the default `host` for an auto-created `port_configs` row — the
/// `host` field isn't consulted anywhere in the live bridging path (the real
/// peer address comes from the SSH client's own `-R` bind address), so this
/// only affects what's displayed, not what's dialed.
pub fn slugify_host(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut last_was_sep = true;
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            last_was_sep = false;
        } else if !last_was_sep {
            out.push('_');
            last_was_sep = true;
        }
    }
    while out.ends_with('_') {
        out.pop();
    }
    if out.is_empty() {
        "localhost".to_string()
    } else {
        out
    }
}
