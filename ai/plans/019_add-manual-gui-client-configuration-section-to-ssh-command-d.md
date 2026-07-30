# Add manual/GUI-client configuration section to SSH Command display

## Context

`SshCommandDisplay.vue` currently only renders a copy-pasteable `ssh …` one-liner plus a ports
table. Many graphical SSH clients (Bitvise, MobaXterm, Termius, PuTTY-family, …) don't take that
one-liner — they ask for the connection fields (host/user/port/key) and each port forward
individually (type, bind address:port, destination host:port). The user wants a collapsed-by-default
section, styled like the existing `cmd-block`, that lists those fields as plain display/copy values
so someone configuring a GUI client by hand can read off exactly what to type, with each field
briefly explained.

## Approach

Everything lives in `frontend/src/components/SshCommandDisplay.vue` (no other files change).

### 1. Refactor forward-flag logic into structured data (DRY, no behavior change)

Currently `portFlag(port)` and the `discoveryFlags` computed each build a raw `-L`/`-R` string
inline. Split the branching from the string formatting so the new section can reuse the same
type/bind-port/to-host/to-port values instead of re-deriving them:

```ts
interface TunnelFields { type: 'Local' | 'Remote'; bindPort: number; toHost: string; toPort: number }

function portFields(port: EntityPort): TunnelFields {
  if (props.entity.entity_type === 'server') {
    return { type: 'Remote', bindPort: port.proxy_port, toHost: port.host, toPort: port.local_port }
  }
  if (port.server_entity_id) {
    const server = servers.value.find(s => s.id === port.server_entity_id)
    return { type: 'Local', bindPort: port.local_port, toHost: server?.hostname ?? port.server_entity_id, toPort: port.proxy_port }
  }
  return { type: 'Local', bindPort: port.local_port, toHost: '<server>', toPort: port.proxy_port }
}
```

`portFlag()` becomes a thin wrapper (`-L`/`-R` prefix + `${bindPort}:${toHost}:${toPort}`) built
from `portFields()`. `discoveryFlags` gets the same treatment — each entry keeps its existing
`flag`/`portId` plus the new structured fields (discovery is always client-side, so always `Local`).

### 2. New computed `manualTunnels`

Combine `enabledPorts` (via `portFields`) and the (now-structured) `discoveryFlags` into one list
for the manual-config table, each row: `{ id, name, type, bindPort, toHost, toPort }`. Reuses
`enabledPorts`/`discoveryFlags`/`portFields` already on the component — no new prop, no API calls.

### 3. Collapsed section markup

Use a native `<details>` element so "collapsed by default" needs no extra state, styled to match
`.cmd-block`/`.cmd-header` (reuse those classes on `<details>`/`<summary>` rather than inventing a
new visual language):

```html
<details class="cmd-block">
  <summary class="cmd-header manual-summary">
    <span class="cmd-label">Manual configuration (graphical clients)</span>
  </summary>
  <div class="manual-body">
    <!-- connection fields, then tunnel fields -->
  </div>
</details>
```

Placed directly after the existing `cmd-block` (still "under the SSH Command section").

### 4. Connection fields block

Each field is a label + a GitHub-style `<code>…</code>` value chip (same visual language as the
`.cmd-flag`/`dp-ports` code pills already in this file) followed by a small copy button, plus a
one-line explanation underneath in muted text (mirroring the existing `.cmd-hint` treatment):

| Field | Value | Note |
|---|---|---|
| User | `{{ entity.id }}` | The SSH username — this entity's ID. |
| Host | `{{ t2tHost }}` | Address of this tunnel2tunnel server. |
| Port | `{{ t2tSshPort }}` | SSH port this server listens on. |
| Password | *(none — key-based auth)* | This server only accepts public-key auth; leave password blank. |
| Private key | `~/.ssh/{{ filename }}` | Path to the key generated/registered for this entity. |

Copy button reuses the `navigator.clipboard.writeText` + timed "Copied!" pattern already used in
`PubkeyInput.vue` (`copyCmd`/`copied`), generalized here as one `copiedField` ref (`string | null`)
+ `copyValue(field, value)`.

**Hover-linking with the SSH command line above.** Each of these fields corresponds to a literal
segment of the one-liner in `.cmd-text` (`ssh -i ~/.ssh/{filename} … {entity.id}@{t2tHost} -p {t2tSshPort}`).
Reuse the same `hoveredPortId`-style approach already driving `.is-hovered` on port flags: add a
`hoveredField` ref (`'user' | 'host' | 'port' | 'key' | null`). Give the relevant existing spans in
`.cmd-text` (`{{ entity.id }}`, `{{ t2tHost }}`, the `-p` port link, `~/.ssh/{{ filename }}`) a
`:class="{ 'is-hovered': hoveredField === 'user' }"` (etc.) plus `@mouseenter`/`@mouseleave` setting
`hoveredField`, and give each manual-config field row the same mouseenter/leave + `:class` binding
keyed to the same field id — so hovering either side highlights both. This is symmetric with how
`hoveredPortId` already links table rows to command flags; it just adds a second, field-keyed ref
for the connection-fields ↔ command-line pairing (port flags already have their own
`hoveredPortId` linkage and don't need this second mechanism).

Then three display-only, disabled checkboxes (not copyable — per the user's own "maybe not the
checkboxes, lol"), each with a one-line explanation:
- Autostart on boot/login — user's own preference, not configured by t2t.
- Autostart tunnel on app launch — recommended: on, so the tunnel comes up without manual action.
- Autorestart on failure — recommended: on, so a dropped connection reconnects automatically.

### 5. Tunnel setup block

A short explainer of the three forward types GUI clients typically expose, then the actual values.

Explanation (once, not per-row — direction is constant for a given entity):
- **Local**: the client opens a port on your machine and forwards connections through the SSH
  server to a destination it can reach — used here because `{{ entity.name }}` is a client
  reaching a server's exposed service.
- **Remote**: the SSH server opens a port on itself and forwards connections back to a destination
  your machine can reach — used here because `{{ entity.name }}` is a server exposing a local
  service through the tunnel.
- **Dynamic**: turns the whole connection into a SOCKS proxy instead of one fixed forward; most GUI
  clients offer it as a third type option, but it isn't something t2t uses today.

Then a table (reuse `.ports-table` styling) driven by `manualTunnels`, columns: Type badge
(Local/Remote), Bind address (`127.0.0.1` — fixed note: "loopback only unless you change it"),
Bind port (`bindPort`), To host (`toHost`), To port (`toPort`), Name — each value cell using the
same `<code>…</code>` chip + copy-button treatment as the connection fields above, plus a per-row
copyable "tunnel string" cell showing `bindPort:toHost:toPort` (the same value already rendered as
the `-L`/`-R` flag in `.cmd-text`), annotated inline with small labels under each colon-separated
segment ("port · to-host · to-port").

**Hover-linking, reusing the existing mechanism.** Port rows already drive `.is-hovered` on their
matching command-line flag via `hoveredPortId` (see `isHovered(portId)` / the `port.id`-keyed
`mouseenter`/`mouseleave` handlers already in the template for both the ports table and the flags
in `.cmd-text`). Each `manualTunnels` row carries the same `id` as its source port (or discovery
port), so giving each new tunnel-table row and its "tunnel string" cell the identical
`@mouseenter="hoveredPortId = row.id"` / `:class="{ 'is-hovered': isHovered(row.id) }"` wiring links
it to the same flag span already highlighted by the existing ports table — no new ref needed here,
just extending `hoveredPortId`'s reach to a third element per port.

### 6. Styles

Add `.manual-body`, `.manual-field-row`, `.manual-field-code` (a `dp-ports`/`cmd-flag`-alike code
chip), `.manual-copy-btn` (mirrors `.btn-copy` from `PubkeyInput.vue`), `.manual-checkbox-row`,
`.manual-tunnel-note` etc. in the existing `<style lang="scss" scoped>` block, following the file's
current hard-coded dark palette (`#0f1117`, `#2d3248`, `#94a3b8`, `#7dd3fc`, …) rather than
introducing new tokens. The `.is-hovered` rule already applied to `.cmd-flag` needs to also apply to
the new code chips/table cells sharing that class, so the existing hover-highlight color is reused
rather than redefined.

## Verification

- `cd frontend && npm run build` (type-checks the refactor to `portFields`/`discoveryFlags`).
- Run the app (`cargo run -p t2t` per CLAUDE.md), open an existing client entity and an existing
  server entity's detail page, expand the new section on each, confirm:
  - Connection fields show correct user/host/port/key path; copy buttons work.
  - Tunnel table shows `Remote` rows for the server entity and `Local` rows for the client entity,
    with bind/to-host/to-port values matching what the existing SSH command line above already
    shows for the same ports.
  - Section is collapsed on page load and toggles via `<summary>` click with no JS errors.
