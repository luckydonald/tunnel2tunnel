# tunnel2tunnel

Self-hosted SSH rendezvous manager. Servers and clients register SSH public keys and establish encrypted tunnels through this node without exposing ports publicly.

## How it works

1. A **server entity** connects with `ssh -R <proxy_port>:localhost:<local_port> <host>` — registering a reverse tunnel.
2. A **client entity** connects with `ssh -L <local_port>:<server_entity_uuid>:<proxy_port> <host>` — opening a forwarded channel.
3. tunnel2tunnel splices the two SSH channels together after verifying access rules.

No ports on the server entity are exposed publicly; all traffic flows through the tunnel2tunnel node.

## Quick start (local dev)

```bash
# Start PostgreSQL
podman run -d --name t2t-pg \
  -e POSTGRES_USER=t2t -e POSTGRES_PASSWORD=t2t_secret -e POSTGRES_DB=tunnel2tunnel \
  -p 5432:5432 docker.io/postgres:18-alpine

# Build frontend
cd frontend && npm install && npm run build && cd ..

# Copy and edit env
cp .env.example .env

# Run (migrations and admin bootstrap applied on startup)
set -a && source .env && set +a
cargo run -p t2t
```

Browse to `http://localhost:3000`. Default login: `admin` / `changeme`.

## Environment variables

| Variable | Default | Description |
|---|---|---|
| `DATABASE_URL` | required | PostgreSQL connection string |
| `HTTP_PORT` | `3000` | HTTP server port |
| `SSH_PORT` | `2222` | SSH rendezvous server port |
| `ADMIN_USERNAME` / `ADMIN_PASSWORD` | — | Bootstrap admin on first run (idempotent) |
| `STATIC_DIR` | `frontend/dist` | Path to compiled Vue SPA; silently skipped if absent |
| `SSH_HOST_KEY_PATH` | `data/ssh_host_key` | Where the SSH host key is persisted. **Mount as a volume** in Docker so the key survives redeploys. |
| `SSH_T2T_KEY_PASSWORD` | — | If set, the stored host key is AES-256-CTR encrypted with this passphrase |
| `FAIL2BAN_LOG_PATH` | — | If set, write sshd-format auth events to this file for fail2ban |
| `RUST_LOG` | — | Tracing filter, e.g. `info` or `tunnel2tunnel_ssh=debug` |

## Docker Compose

```bash
cp .env.example .env
# edit .env — set ADMIN_PASSWORD and optionally SSH_T2T_KEY_PASSWORD
docker compose up -d
```

The `t2t_data` volume at `/data` persists the SSH host key across restarts. Without it, every restart triggers a "REMOTE HOST IDENTIFICATION HAS CHANGED!" warning on all clients.

## Coolify deployment

Use `docker-compose.coolify.yml`. Coolify injects `SERVICE_FQDN_TUNNEL2TUNNEL_3000`, `SERVICE_USER_POSTGRES`, `SERVICE_PASSWORD_POSTGRES`, and `SERVICE_PASSWORD_ADMIN` automatically. Add `SSH_T2T_KEY_PASSWORD` as a custom environment variable in the Coolify UI.

## fail2ban

A filter is provided at `contrib/fail2ban/filter.d/tunnel2tunnel.conf`. Set `FAIL2BAN_LOG_PATH` to a file path and point your fail2ban jail at it.
