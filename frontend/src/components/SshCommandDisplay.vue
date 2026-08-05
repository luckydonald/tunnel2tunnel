<script setup lang="ts">
import { ref, computed, nextTick } from 'vue'
import type { Entity, PortConfig, PortSubscription } from '@/api/entities'

export interface OwnSubscriptionRow {
  subscription: PortSubscription
  service: PortConfig
  ownerId: string
  ownerName: string | null
}

const props = withDefaults(defineProps<{
  entity: Entity
  ports: PortConfig[]
  /** This entity's own port_subscriptions, each paired with the service + owning entity it targets. */
  subscriptions?: OwnSubscriptionRow[]
  filename?: string
  t2tHost?: string
  t2tSshPort?: number
}>(), {
  filename: 't2t_key',
  t2tSshPort: 2222,
  subscriptions: () => [],
})

const hoveredPortId = ref<string | null>(null)
const nonInteractive = ref(false)
const t2tHost = computed(() => props.t2tHost ?? window.location.hostname)

// Connection-field hover linking between the manual-config block and the ssh command line
type ConnectionField = 'user' | 'host' | 'port' | 'key'
const hoveredField = ref<ConnectionField | null>(null)
function isFieldHovered(field: ConnectionField): boolean {
  return hoveredField.value === field
}

// Structured -L/-R forward fields, shared by the ssh command line and the manual-config table
interface TunnelFields { type: 'Local' | 'Remote'; bindPort: number; toHost: string; toPort: number }
interface TunnelRow extends TunnelFields { id: string; name: string; flag: string }

function flagString(fields: TunnelFields): string {
  const prefix = fields.type === 'Remote' ? '-R' : '-L'
  return `${prefix} ${fields.bindPort}:${fields.toHost}:${fields.toPort}`
}

// Every owned, enabled service contributes a -R flag (this entity offers it).
const ownedRows = computed((): TunnelRow[] =>
  props.ports.filter(p => p.enabled).map(port => {
    const fields: TunnelFields = { type: 'Remote', bindPort: port.proxy_port, toHost: port.host, toPort: port.local_port }
    return { id: port.id, name: port.name, ...fields, flag: flagString(fields) }
  }),
)

// Every own, enabled subscription contributes a -L flag (this entity subscribes to it).
const subscribedRows = computed((): TunnelRow[] =>
  props.subscriptions.filter(s => s.subscription.enabled).map(s => {
    const fields: TunnelFields = {
      type: 'Local',
      bindPort: s.subscription.subscriber_local_port,
      toHost: s.ownerName ?? s.ownerId,
      toPort: s.service.proxy_port,
    }
    return { id: s.subscription.id, name: s.service.name, ...fields, flag: flagString(fields) }
  }),
)

// Combined rows for the ssh command line and the manual/GUI-client tunnel-setup table.
const allRows = computed((): TunnelRow[] => [...ownedRows.value, ...subscribedRows.value])

function isHovered(rowId: string): boolean {
  return hoveredPortId.value === rowId
}

// Clicking a -L/-R chip expands the manual-config summary and scrolls/highlights its row.
const detailsRef = ref<HTMLDetailsElement | null>(null)
const rowRefs = new Map<string, HTMLElement>()
function setRowRef(rowId: string, el: Element | null): void {
  if (el) rowRefs.set(rowId, el as HTMLElement)
  else rowRefs.delete(rowId)
}
async function expandToRow(rowId: string): Promise<void> {
  if (detailsRef.value) detailsRef.value.open = true
  hoveredPortId.value = rowId
  await nextTick()
  rowRefs.get(rowId)?.scrollIntoView({ behavior: 'smooth', block: 'center' })
}

// Copy-to-clipboard for the manual-config code chips
const copiedField = ref<string | null>(null)
async function copyValue(field: string, value: string): Promise<void> {
  await navigator.clipboard.writeText(value)
  copiedField.value = field
  setTimeout(() => { if (copiedField.value === field) copiedField.value = null }, 2000)
}
</script>

<template>
  <div class="ssh-cmd-display">

    <!-- SSH command block -->
    <div class="cmd-block">
      <div class="cmd-header">
        <span class="cmd-label">SSH command</span>
        <label class="checkbox-label cmd-toggle">
          <input type="checkbox" v-model="nonInteractive" />
          Non-interactive mode
        </label>
        <span class="cmd-hint">Disables text channel providing status updates.</span>
      </div>
      <pre class="cmd-text">ssh <template v-if="nonInteractive">-N \
  </template>-i <span class="cmd-flag" :class="{ 'is-hovered': isFieldHovered('key') }" @mouseenter="hoveredField = 'key'" @mouseleave="hoveredField = null">~/.ssh/{{ filename }}</span><template v-for="row in allRows" :key="row.id"> \
  <span
    class="cmd-flag cmd-flag-clickable"
    :class="{ 'is-hovered': isHovered(row.id) }"
    @mouseenter="hoveredPortId = row.id"
    @mouseleave="hoveredPortId = null"
    @click="expandToRow(row.id)"
    :title="`Show ${row.name} in manual configuration`"
  >{{ row.flag }}</span></template> \
  <span class="cmd-flag" :class="{ 'is-hovered': isFieldHovered('user') }" @mouseenter="hoveredField = 'user'" @mouseleave="hoveredField = null">{{ entity.id }}</span>@<span class="cmd-flag" :class="{ 'is-hovered': isFieldHovered('host') }" @mouseenter="hoveredField = 'host'" @mouseleave="hoveredField = null">{{ t2tHost }}</span> \
  -p <RouterLink to="/settings" class="port-link" :class="{ 'is-hovered': isFieldHovered('port') }" @mouseenter="hoveredField = 'port'" @mouseleave="hoveredField = null">{{ t2tSshPort }}</RouterLink></pre>
    </div>

    <!-- Manual configuration for graphical (GUI) SSH clients -->
    <details class="cmd-block">
      <summary class="cmd-header manual-summary">
        <span class="cmd-label">Manual configuration (graphical clients)</span>
        <span class="cmd-hint">For clients like Bitvise, MobaXterm, Termius, PuTTY, etc.</span>
      </summary>
      <div class="manual-body">
        <div class="manual-group">
          <div class="manual-group-title">Connection</div>

          <div class="manual-field-row">
            <span class="manual-field-label">User</span>
            <code
              class="cmd-flag manual-field-code"
              :class="{ 'is-hovered': isFieldHovered('user') }"
              @mouseenter="hoveredField = 'user'"
              @mouseleave="hoveredField = null"
            >{{ entity.id }}</code>
            <button type="button" class="btn-copy" :class="{ copied: copiedField === 'user' }" @click="copyValue('user', entity.id)">
              {{ copiedField === 'user' ? 'Copied!' : 'Copy' }}
            </button>
          </div>
          <p class="manual-field-note">The SSH username — this entity's ID.</p>

          <div class="manual-field-row">
            <span class="manual-field-label">Host</span>
            <code
              class="cmd-flag manual-field-code"
              :class="{ 'is-hovered': isFieldHovered('host') }"
              @mouseenter="hoveredField = 'host'"
              @mouseleave="hoveredField = null"
            >{{ t2tHost }}</code>
            <button type="button" class="btn-copy" :class="{ copied: copiedField === 'host' }" @click="copyValue('host', t2tHost)">
              {{ copiedField === 'host' ? 'Copied!' : 'Copy' }}
            </button>
          </div>
          <p class="manual-field-note">Address of this tunnel2tunnel server.</p>

          <div class="manual-field-row">
            <span class="manual-field-label">Port</span>
            <code
              class="cmd-flag manual-field-code"
              :class="{ 'is-hovered': isFieldHovered('port') }"
              @mouseenter="hoveredField = 'port'"
              @mouseleave="hoveredField = null"
            >{{ t2tSshPort }}</code>
            <button type="button" class="btn-copy" :class="{ copied: copiedField === 'port' }" @click="copyValue('port', String(t2tSshPort))">
              {{ copiedField === 'port' ? 'Copied!' : 'Copy' }}
            </button>
          </div>
          <p class="manual-field-note">SSH port this server listens on.</p>

          <div class="manual-field-row">
            <span class="manual-field-label">Password</span>
            <code class="cmd-flag manual-field-code manual-field-na">— (none)</code>
          </div>
          <p class="manual-field-note">This server only accepts public-key auth; leave the password field blank.</p>

          <div class="manual-field-row">
            <span class="manual-field-label">Private key</span>
            <code
              class="cmd-flag manual-field-code"
              :class="{ 'is-hovered': isFieldHovered('key') }"
              @mouseenter="hoveredField = 'key'"
              @mouseleave="hoveredField = null"
            >~/.ssh/{{ filename }}</code>
            <button type="button" class="btn-copy" :class="{ copied: copiedField === 'key' }" @click="copyValue('key', '~/.ssh/' + filename)">
              {{ copiedField === 'key' ? 'Copied!' : 'Copy' }}
            </button>
          </div>
          <p class="manual-field-note">Path to the key generated/registered for this entity.</p>

          <div class="manual-checkbox-row">
            <label class="checkbox-label"><input type="checkbox" disabled /> Autostart on boot/login</label>
            <p class="manual-field-note">Your own preference — not configured by t2t.</p>
          </div>
          <div class="manual-checkbox-row">
            <label class="checkbox-label"><input type="checkbox" checked disabled /> Autostart tunnel on app launch</label>
            <p class="manual-field-note">Recommended: on, so the tunnel comes up without manual action.</p>
          </div>
          <div class="manual-checkbox-row">
            <label class="checkbox-label"><input type="checkbox" checked disabled /> Autorestart on failure</label>
            <p class="manual-field-note">Recommended: on, so a dropped connection reconnects automatically.</p>
          </div>
        </div>

        <div class="manual-group">
          <div class="manual-group-title">Tunnel setup</div>
          <p class="manual-type-explainer">
            <strong>Remote</strong> — services this entity offers: the SSH server opens a port on itself and forwards connections back to this machine.
            <strong>Local</strong> — services this entity subscribes to: this machine opens a port and forwards connections through the SSH server to the owning entity.
            <strong>Dynamic</strong> — turns the whole connection into a SOCKS proxy instead of one fixed forward; most GUI clients offer it as a third type, but t2t doesn't use it.
          </p>

          <table v-if="allRows.length" class="ports-table manual-tunnel-table">
            <thead>
              <tr>
                <th>Type</th>
                <th>Bind address</th>
                <th>Bind port</th>
                <th>To host</th>
                <th>To port</th>
                <th>Tunnel string</th>
                <th>Name</th>
              </tr>
            </thead>
            <tbody>
              <tr
                v-for="row in allRows"
                :key="row.id"
                :ref="el => setRowRef(row.id, el as Element | null)"
                :class="{ 'is-hovered': isHovered(row.id) }"
                @mouseenter="hoveredPortId = row.id"
                @mouseleave="hoveredPortId = null"
              >
                <td><span class="dp-badge" :class="row.type === 'Remote' ? 'enabled' : 'auto'">{{ row.type }}</span></td>
                <td><code>127.0.0.1</code></td>
                <td><code>{{ row.bindPort }}</code></td>
                <td><code>{{ row.toHost }}</code></td>
                <td><code>{{ row.toPort }}</code></td>
                <td>
                  <code class="manual-tunnel-string">{{ row.bindPort }}:{{ row.toHost }}:{{ row.toPort }}</code>
                  <div class="manual-tunnel-legend">port · to-host · to-port</div>
                </td>
                <td>{{ row.name }}</td>
              </tr>
            </tbody>
          </table>
          <p v-else class="no-ports">No enabled tunnels.</p>
          <p class="manual-field-note">Bind address defaults to <code>127.0.0.1</code> (loopback only) unless you change it in your client.</p>
        </div>
      </div>
    </details>

  </div>
</template>

<style lang="scss" scoped>
.ssh-cmd-display {
  display: flex;
  flex-direction: column;
  gap: 1rem;
}

// ── SSH command block ─────────────────────────────────────────────────────────

.cmd-block {
  background: #0f1117;
  border: 1px solid #2d3248;
  border-radius: 6px;
  overflow: hidden;
}

.cmd-header {
  display: flex;
  align-items: center;
  gap: 1rem;
  padding: 0.5rem 0.875rem;
  border-bottom: 1px solid #2d3248;
  background: #1a1d27;
}

.cmd-label {
  font-size: 0.75rem;
  text-transform: uppercase;
  letter-spacing: 0.08em;
  color: #64748b;
  font-weight: 600;
}

.cmd-hint {
  font-size: 0.8125rem;
  color: #94a3b8;

  code {
    background: #2d3248;
    padding: 0.1em 0.3em;
    border-radius: 3px;
    font-size: 0.875em;
  }
}

.checkbox-label {
  display: flex;
  align-items: center;
  gap: 0.375rem;
  color: #94a3b8;
  font-size: 0.875rem;
  cursor: pointer;
}

.cmd-toggle {
  margin-left: auto;
}

.cmd-text {
  margin: 0;
  padding: 0.875rem;
  font-family: 'Fira Code', 'Cascadia Code', ui-monospace, monospace;
  font-size: 0.8125rem;
  line-height: 1.6;
  color: #94a3b8;
  white-space: pre-wrap;
  word-break: break-all;
}

.port-link {
  color: #94a3b8;
  text-decoration: none;
  border-bottom: 1px dotted #4f6ef7;
  &:hover, &.is-hovered { color: #7dd3fc; border-bottom-style: solid; }
  &.is-hovered { background: rgba(79, 110, 247, 0.25); }
}

.cmd-flag {
  color: #7dd3fc;
  border-radius: 2px;
  padding: 0.1em 0;
  cursor: default;
  text-decoration: none;
  transition: background 0.1s;

  &.is-hovered {
    background: rgba(79, 110, 247, 0.25);
    color: #93c5fd;
  }

  // RouterLink variant: show cursor pointer and subtle underline
  &[href] {
    cursor: pointer;
    border-bottom: 1px dotted #4f6ef7;
    &:hover { border-bottom-style: solid; }
  }
}

// ── Manual/GUI-client configuration ────────────────────────────────────────────

.manual-summary {
  cursor: pointer;
  list-style: none;
  user-select: none;

  &::-webkit-details-marker { display: none; }

  &::before {
    content: '▸';
    color: #64748b;
    font-size: 0.75rem;
    margin-right: 0.25rem;
  }
}

details[open] > .manual-summary::before {
  content: '▾';
}

.manual-body {
  padding: 0.875rem;
  display: flex;
  flex-direction: column;
  gap: 1.25rem;
}

.manual-group-title {
  font-size: 0.75rem;
  text-transform: uppercase;
  letter-spacing: 0.08em;
  color: #64748b;
  font-weight: 600;
  margin-bottom: 0.625rem;
}

.manual-field-row {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  margin-bottom: 0.125rem;
}

.manual-field-label {
  width: 90px;
  flex-shrink: 0;
  font-size: 0.8125rem;
  color: #94a3b8;
}

.manual-field-code {
  background: #1a1d27;
  padding: 0.15em 0.4em;
  border-radius: 3px;
  font-size: 0.8125rem;
}

.manual-field-na {
  color: #64748b;
}

.manual-field-note {
  margin: 0 0 0.75rem calc(90px + 0.5rem);
  font-size: 0.75rem;
  color: #64748b;

  code {
    background: #1a1d27;
    padding: 0.1em 0.35em;
    border-radius: 3px;
  }
}

.manual-checkbox-row {
  margin-bottom: 0.25rem;

  .manual-field-note {
    margin-left: 0;
  }
}

.manual-type-explainer {
  font-size: 0.8125rem;
  color: #94a3b8;
  line-height: 1.6;
  margin: 0 0 1rem;

  strong {
    color: #cbd5e1;
  }
}

.manual-tunnel-string {
  background: #1a1d27;
  padding: 0.15em 0.4em;
  border-radius: 3px;
  font-size: 0.8125rem;
  color: #7dd3fc;
  display: block;
}

.manual-tunnel-legend {
  font-size: 0.6875rem;
  color: #475569;
  margin-top: 0.2rem;
}

.btn-copy {
  padding: 0.2rem 0.6rem;
  background: none;
  border: 1px solid #2d3248;
  border-radius: 3px;
  color: #94a3b8;
  font-size: 0.75rem;
  cursor: pointer;
  white-space: nowrap;

  &:hover  { color: #e2e8f0; border-color: #4f6ef7; }
  &.copied { color: #6ee7b7; border-color: #34d399; }
}

// ── Ports table ───────────────────────────────────────────────────────────────

.ports-table {
  width: 100%;
  border-collapse: collapse;
  font-size: 0.875rem;

  th {
    text-align: left;
    padding: 0.5rem 0.75rem;
    border-bottom: 1px solid #2d3248;
    color: #64748b;
    font-weight: 500;
    font-size: 0.75rem;
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }

  tr {
    cursor: default;
    transition: background 0.1s;

    &.is-hovered {
      background: rgba(79, 110, 247, 0.08);
    }

    &.is-disabled {
      opacity: 0.45;
    }
  }

  td {
    padding: 0.5rem 0.75rem;
    border-bottom: 1px solid #1e2235;
    color: #e2e8f0;

    code {
      background: #1a1d27;
      padding: 0.15em 0.4em;
      border-radius: 3px;
      font-size: 0.875em;
    }
  }

  .td-enabled {
    color: #64748b;
    font-size: 0.75rem;
  }
}

.no-ports {
  margin: 0;
  color: #64748b;
  font-size: 0.875rem;
}
</style>
