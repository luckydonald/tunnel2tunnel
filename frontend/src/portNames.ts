// Client-side mirror of the backend's well-known-port name guesses
// (crates/tunnel2tunnel-core/src/port_names.rs::guess_service_name).
// Keep this list in sync with that file.

const PORT_NAMES: Record<number, string> = {
  21: 'FTP',
  22: 'SSH',
  23: 'Telnet',
  25: 'SMTP',
  53: 'DNS',
  80: 'HTTP',
  110: 'POP3',
  143: 'IMAP',
  443: 'HTTPS',
  445: 'SMB',
  465: 'SMTPS',
  587: 'SMTP (submission)',
  993: 'IMAPS',
  995: 'POP3S',
  1433: 'MSSQL',
  1521: 'Oracle DB',
  2049: 'NFS',
  3000: 'Node.js (dev)',
  3001: 'Grafana',
  3306: 'MySQL / MariaDB',
  3389: 'RDP',
  5000: 'Flask / dev',
  5432: 'PostgreSQL',
  5900: 'VNC',
  6379: 'Redis',
  6443: 'Kubernetes API',
  7777: 'Game server',
  8000: 'HTTP (dev)',
  8006: 'Proxmox',
  8080: 'HTTP (alt)',
  8081: 'HTTP (alt 2)',
  8096: 'Jellyfin',
  8123: 'Home Assistant',
  8384: 'Syncthing',
  8443: 'HTTPS (alt)',
  8880: 'phpMyAdmin',
  8888: 'Jupyter',
  9000: 'Portainer / MinIO',
  9090: 'Prometheus',
  9091: 'Transmission',
  9100: 'Node exporter',
  9200: 'Elasticsearch',
  11211: 'Memcached',
  25565: 'Minecraft',
  27015: 'Steam / Source',
  27017: 'MongoDB',
  32400: 'Plex',
}

/** Best-effort guess of a human-friendly service name for a well-known port, or `null` if nothing matches. */
export function guessServiceName(port: number): string | null {
  return PORT_NAMES[port] ?? null
}
