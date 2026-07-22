# Manual testing — two-connection tunnel

End-to-end test that proves the rendezvous actually bridges two independent
SSH connections: one registers a remote forward (`-R`), the other consumes it
(`-L`), and traffic flows through t2t between them.

All commands assume you're in the repo root unless noted otherwise.

## 1. Postgres (PG18 required — `uuidv7()` is a native PG18 builtin)

If you already have a PG18 server running (e.g. via Postgres.app, which can
run multiple server versions side by side — check with
`ps aux | grep postgres` for a `.../Versions/18/bin/postgres` process), just
create the role/db on it:

```bash
psql -p 5432 -U "$USER" -d postgres <<'SQL'
DO $$ BEGIN
  IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 't2t') THEN
    CREATE ROLE t2t LOGIN PASSWORD 't2t_secret';
  END IF;
END $$;
SELECT 'CREATE DATABASE tunnel2tunnel OWNER t2t'
WHERE NOT EXISTS (SELECT FROM pg_database WHERE datname = 'tunnel2tunnel')\gexec
SQL
```

Otherwise use Podman per `CLAUDE.md`:

```bash
podman run -d --name t2t-pg \
  -e POSTGRES_USER=t2t -e POSTGRES_PASSWORD=t2t_secret -e POSTGRES_DB=tunnel2tunnel \
  -p 5432:5432 docker.io/postgres:18-alpine
```

## 2. Build

```bash
cd frontend && npm install && npm run build && cd ..
cargo build -p t2t
```

## 3. Scratch directory

Everything for this test (SSH host key, generated keypairs, logs, the
fixture web root) lives under a throwaway `/tmp` dir so it never touches the
repo:

```bash
mkdir -p /tmp/t2t-test-run
```

## 4. Run the server

```bash
DATABASE_URL=postgres://t2t:t2t_secret@localhost:5432/tunnel2tunnel \
ADMIN_USERNAME=admin ADMIN_PASSWORD=changeme \
HTTP_PORT=3000 SSH_PORT=2222 STATIC_DIR=frontend/dist \
SSH_HOST_KEY_PATH=/tmp/t2t-test-run/ssh_host_key \
RUST_LOG=info \
cargo run -p t2t
```

(Run in the background with `nohup ... > /tmp/t2t-test-run/server.log 2>&1 &
disown` if you want the shell back.)

Wait for `HTTP server listening on 0.0.0.0:3000` in the log before continuing.

## 5. Log in and grab a session cookie

```bash
cd /tmp/t2t-test-run
curl -s -c cookies.txt -X POST http://localhost:3000/api/auth/login \
  -H "Content-Type: application/json" \
  -d '{"username":"admin","password":"changeme"}'
```

## 6. Create the two entities

One `server` entity (will run `-R`, registering a port) and one `client`
entity (will run `-L`, consuming it):

```bash
SERVER_ENTITY=$(curl -s -b cookies.txt -X POST http://localhost:3000/api/entities \
  -H "Content-Type: application/json" \
  -d '{"entity_type":"server","name":"test-server"}')
CLIENT_ENTITY=$(curl -s -b cookies.txt -X POST http://localhost:3000/api/entities \
  -H "Content-Type: application/json" \
  -d '{"entity_type":"client","name":"test-client"}')

SERVER_ID=$(echo "$SERVER_ENTITY" | grep -o '"id":"[^"]*"' | head -1 | sed -E 's/"id":"([^"]*)"/\1/')
CLIENT_ID=$(echo "$CLIENT_ENTITY" | grep -o '"id":"[^"]*"' | head -1 | sed -E 's/"id":"([^"]*)"/\1/')
echo "server=$SERVER_ID client=$CLIENT_ID"
```

## 7. Generate a keypair per entity and register the public keys

```bash
ssh-keygen -t ed25519 -f server_key -N "" -q -C "test-server"
ssh-keygen -t ed25519 -f client_key -N "" -q -C "test-client"
chmod 600 server_key client_key

SERVER_ALGO=$(awk '{print $1}' server_key.pub)
SERVER_KEYDATA=$(awk '{print $2}' server_key.pub)
CLIENT_ALGO=$(awk '{print $1}' client_key.pub)
CLIENT_KEYDATA=$(awk '{print $2}' client_key.pub)

curl -s -b cookies.txt -X POST "http://localhost:3000/api/entities/$SERVER_ID/keys" \
  -H "Content-Type: application/json" \
  -d "{\"algorithm\":\"$SERVER_ALGO\",\"key_data\":\"$SERVER_KEYDATA\",\"comment\":\"test-server\"}"

curl -s -b cookies.txt -X POST "http://localhost:3000/api/entities/$CLIENT_ID/keys" \
  -H "Content-Type: application/json" \
  -d "{\"algorithm\":\"$CLIENT_ALGO\",\"key_data\":\"$CLIENT_KEYDATA\",\"comment\":\"test-client\"}"
```

`key_data` is just the base64 blob (2nd whitespace-separated field) of the
`.pub` file — not the whole `authorized_keys`-style line.

## 8. Grant the client access to the server

```bash
curl -s -b cookies.txt -X POST "http://localhost:3000/api/entities/$SERVER_ID/access" \
  -H "Content-Type: application/json" \
  -d "{\"subject_type\":\"entity\",\"subject_entity_id\":\"$CLIENT_ID\"}"
```

`subject_type` can also be `all_mine` (any entity owned by the same user),
`all_user_entities`, or `public_lite` — see `entity_access.rs`.

## 9. Stand up a local fixture service

This is the "real" service being tunneled to — anything listening on a local
port works. Here it's a static file server:

```bash
mkdir -p webroot && echo "hello from the tunneled service" > webroot/index.html
python3 -m http.server 8765 --directory webroot &
```

## 10. Connection 1 — the "server" entity registers the forward

```bash
ssh -N -R 9000:127.0.0.1:8765 \
  -i server_key -p 2222 \
  -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null \
  -o ExitOnForwardFailure=yes \
  server@localhost &
```

- `9000` is the **proxy port** — the key t2t uses to route to this
  connection. It has no relation to any port on the real machine.
- `127.0.0.1:8765` is what the server entity's ssh client connects to
  *locally* once t2t asks it to open a forwarded channel.
- The SSH username (`server@`) is arbitrary — auth is by public key fingerprint
  only, the username is not checked.

Check the backend log for `server registered port ... proxy_port=9000`.

## 11. Connection 2 — the "client" entity consumes it

```bash
ssh -N -L "9001:${SERVER_ID}:9000" \
  -i client_key -p 2222 \
  -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null \
  -o ExitOnForwardFailure=yes \
  client@localhost &
```

- The host in `-L local_port:HOST:proxy_port` is the **target entity's
  UUID** (or an `entity_access.hostname` alias, if one was set on the access
  rule in step 8). This is how t2t knows which server entity/port to bridge
  to.
- `9000` here must match the proxy port from step 10.
- `9001` is just a local port on your machine to test against.

Check the backend log for `direct-tcpip: bridge established`.

## 12. Verify traffic actually flows end-to-end

```bash
curl -s http://127.0.0.1:9001/
# hello from the tunneled service
```

If this returns the fixture content, both SSH connections and the t2t
rendezvous in between are working correctly. Repeat the `curl` a couple of
times — the two SSH connections should stay up and keep serving without
needing to reconnect.

## Cleanup

```bash
pkill -f "R 9000:127.0.0.1:8765"
pkill -f "L 9001:"
pkill -f "http.server 8765"
pkill -f "target/debug/t2t"
rm -rf /tmp/t2t-test-run
```

## Known gotcha this test caught

`channel_open_direct_tcpip` (`crates/tunnel2tunnel-ssh/src/lib.rs`) must ask
the *registering* SSH connection to open its forwarded-tcpip channel using
the **address that connection originally passed to `tcpip_forward`** (e.g.
`"localhost"` from `-R 9000:...`), not the hostname the `-L` side requested
(e.g. the target entity's UUID). OpenSSH matches incoming forwarded-tcpip
requests against its own registered `(address, port)` table, so passing the
wrong address makes OpenSSH reject the channel with
`WARNING: Server requests forwarding for unknown listen_port <port>` and the
whole connection drops. The fix stores the registered address alongside the
`Handle` in `server_slots` when `tcpip_forward` is called, and reuses it in
`channel_open_direct_tcpip` instead of the client-supplied hostname.
