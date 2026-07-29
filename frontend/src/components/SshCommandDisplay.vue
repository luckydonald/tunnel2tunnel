<script setup lang="ts">
import { ref, computed } from 'vue'
import { RouterLink } from 'vue-router'
import type { Entity, EntityPort, ReachableServer } from '@/api/entities'

const props = withDefaults(defineProps<{
  entity: Entity
  ports: EntityPort[]
  filename?: string
  t2tHost?: string
  t2tSshPort?: number
  reachableServers?: ReachableServer[]
}>(), {
  filename: 't2t_key',
  t2tSshPort: 2222,
  reachableServers: () => [],
})

const emit = defineEmits<{
  'discovery-state-change': [serverPortId: string, state: 'auto' | 'enabled' | 'disabled', localPort?: number]
}>()

const hoveredPortId = ref<string | null>(null)
const nonInteractive = ref(false)
const t2tHost = computed(() => props.t2tHost ?? window.location.hostname)
const servers = computed(() => props.reachableServers ?? [])

// Client-side EntityPort IDs managed by "enabled" discovery rules
const enabledDiscoveryPortIds = computed(() => {
  const ids = new Set<string>()
  for (const server of servers.value) {
    for (const dp of server.ports) {
      if (dp.discovery_state === 'enabled' && dp.client_port_id) {
        ids.add(dp.client_port_id)
      }
    }
  }
  return ids
})

// Own ports that are not managed by a discovery rule (the "normal" ones)
const enabledPorts = computed(() =>
  props.ports.filter(p => p.enabled && !enabledDiscoveryPortIds.value.has(p.id))
)

// Extra -L flags contributed by auto/enabled discovered ports
interface DiscoveryFlag { flag: string; portId: string }
const discoveryFlags = computed((): DiscoveryFlag[] => {
  const flags: DiscoveryFlag[] = []
  for (const server of servers.value) {
    const display = server.hostname ?? server.id
    for (const dp of server.ports) {
      if (dp.discovery_state === null) {
        flags.push({ flag: `-L ${dp.local_port}:${display}:${dp.proxy_port}`, portId: dp.id })
      } else if (dp.discovery_state === 'enabled' && dp.client_port_id) {
        const clientPort = props.ports.find(p => p.id === dp.client_port_id)
        const localPort = clientPort?.local_port ?? dp.local_port
        flags.push({ flag: `-L ${localPort}:${display}:${dp.proxy_port}`, portId: dp.id })
      }
    }
  }
  return flags
})

function portFlag(port: EntityPort): string {
  if (props.entity.entity_type === 'server') {
    return `-R ${port.proxy_port}:${port.host}:${port.local_port}`
  }
  if (port.server_entity_id) {
    const server = servers.value.find(s => s.id === port.server_entity_id)
    const display = server?.hostname ?? port.server_entity_id
    return `-L ${port.local_port}:${display}:${port.proxy_port}`
  }
  return `-L ${port.local_port}:<server>:${port.proxy_port}`
}

function isHovered(portId: string): boolean {
  return hoveredPortId.value === portId
}

// Inline "pin to enabled" form state
const pinningPortId = ref<string | null>(null)
const pinLocalPort = ref<number>(0)

function startPin(portId: string, defaultLocalPort: number): void {
  pinningPortId.value = portId
  pinLocalPort.value = defaultLocalPort
}

function confirmPin(portId: string): void {
  emit('discovery-state-change', portId, 'enabled', pinLocalPort.value)
  pinningPortId.value = null
}

function cancelPin(): void {
  pinningPortId.value = null
}

function toggleDiscovery(portId: string, currentState: string | null): void {
  if (currentState === 'disabled') {
    emit('discovery-state-change', portId, 'auto')
  } else {
    emit('discovery-state-change', portId, 'disabled')
  }
}
</script>

<template>
  <div class="ssh-cmd-display">

    <!-- Discovery section: reachable server ports (client entities only) -->
    <div v-if="entity.entity_type === 'client' && servers.length" class="discovery-section">
      <div class="discovery-header">Authorized server ports</div>
      <div v-for="server in servers" :key="server.id" class="discovery-server">
        <div class="server-name">
          {{ server.name ?? server.id.slice(0, 13) + '…' }}
          <span v-if="server.hostname" class="server-hostname">{{ server.hostname }}</span>
          <code v-else class="server-uuid">{{ server.id }}</code>
        </div>
        <div v-if="server.ports.length" class="discovery-ports">
          <div
            v-for="dp in server.ports"
            :key="dp.id"
            class="discovery-port-row"
            :class="{ 'is-disabled': dp.discovery_state === 'disabled', 'is-auto': dp.discovery_state === null, 'is-enabled': dp.discovery_state === 'enabled' }"
          >
            <input
              type="checkbox"
              :checked="dp.discovery_state !== 'disabled'"
              class="discovery-check"
              @change="toggleDiscovery(dp.id, dp.discovery_state)"
            />
            <code class="dp-ports">{{ dp.local_port }}→{{ dp.proxy_port }}</code>
            <span class="dp-name">{{ dp.name ?? '' }}</span>
            <span v-if="dp.discovery_state === 'enabled'" class="dp-badge enabled">pinned</span>
            <span v-else-if="dp.discovery_state === 'disabled'" class="dp-badge disabled">off</span>
            <span v-else class="dp-badge auto">auto</span>

            <!-- Pin form (auto → enabled) -->
            <template v-if="dp.discovery_state === null">
              <template v-if="pinningPortId === dp.id">
                <input
                  v-model.number="pinLocalPort"
                  type="number"
                  class="pin-input"
                  min="1"
                  max="65535"
                  placeholder="local port"
                  @keyup.enter="confirmPin(dp.id)"
                  @keyup.esc="cancelPin"
                />
                <button class="btn-pin-ok" @click="confirmPin(dp.id)">Pin</button>
                <button class="btn-icon" @click="cancelPin">✕</button>
              </template>
              <button v-else class="btn-icon btn-pin" title="Pin as fixed port" @click="startPin(dp.id, dp.local_port)">
                📌
              </button>
            </template>

            <!-- Reset to auto -->
            <button
              v-if="dp.discovery_state !== null"
              class="btn-icon btn-reset"
              title="Reset to auto"
              @click="emit('discovery-state-change', dp.id, 'auto')"
            ><kbd>x</kbd></button>
          </div>
        </div>
        <p v-else class="no-server-ports">No enabled ports.</p>
      </div>
    </div>

    <!-- SSH command block -->
    <div class="cmd-block">
      <div class="cmd-header">
        <span class="cmd-label">SSH command</span>
        <span class="cmd-hint" v-if="entity.entity_type === 'client' && !servers.length">
          Replace <code>&lt;server&gt;</code> with the server entity ID
        </span>
        <label class="checkbox-label cmd-toggle">
          <input type="checkbox" v-model="nonInteractive" />
          Non-interactive mode
        </label>
        <span class="cmd-hint">Disables text channel providing status updates.</span>
      </div>
      <pre class="cmd-text">ssh <template v-if="nonInteractive">-N \
  </template>-i ~/.ssh/{{ filename }}<template v-for="port in enabledPorts" :key="port.id"> \
  <RouterLink
    v-if="entity.entity_type === 'client' && port.server_entity_id"
    :to="'/entities/' + port.server_entity_id + '#ports'"
    class="cmd-flag"
    :class="{ 'is-hovered': isHovered(port.id) }"
    @mouseenter="hoveredPortId = port.id"
    @mouseleave="hoveredPortId = null"
  >{{ portFlag(port) }}</RouterLink><span
    v-else
    class="cmd-flag"
    :class="{ 'is-hovered': isHovered(port.id) }"
    @mouseenter="hoveredPortId = port.id"
    @mouseleave="hoveredPortId = null"
  >{{ portFlag(port) }}</span></template><template v-for="df in discoveryFlags" :key="df.portId"> \
  <span class="cmd-flag cmd-flag-discovery">{{ df.flag }}</span></template> \
  {{ entity.id }}@{{ t2tHost }} \
  -p <RouterLink to="/settings" class="port-link">{{ t2tSshPort }}</RouterLink></pre>
    </div>

    <table v-if="ports.length" class="ports-table">
      <thead>
        <tr>
          <th></th>
          <th v-if="entity.entity_type === 'server'">Proxy port</th>
          <th v-else>Local port</th>
          <th v-if="entity.entity_type === 'server'">Local port</th>
          <th v-else>Server proxy port</th>
          <th>Name</th>
        </tr>
      </thead>
      <tbody>
        <tr
          v-for="port in ports"
          :key="port.id"
          :class="{ 'is-hovered': isHovered(port.id), 'is-disabled': !port.enabled }"
          @mouseenter="hoveredPortId = port.id"
          @mouseleave="hoveredPortId = null"
        >
          <td class="td-enabled">{{ port.enabled ? '●' : '○' }}</td>
          <td>
            <code>{{ entity.entity_type === 'server' ? port.proxy_port : port.local_port }}</code>
          </td>
          <td>
            <code>{{ entity.entity_type === 'server' ? port.local_port : port.proxy_port }}</code>
          </td>
          <td>{{ port.name ?? '—' }}</td>
        </tr>
      </tbody>
    </table>
    <p v-else class="no-ports">No ports configured.</p>
  </div>
</template>

<style lang="scss" scoped>
.ssh-cmd-display {
  display: flex;
  flex-direction: column;
  gap: 1rem;
}

// ── Discovery section ─────────────────────────────────────────────────────────

.discovery-section {
  border: 1px solid #2d3248;
  border-radius: 6px;
  overflow: hidden;
}

.discovery-header {
  padding: 0.5rem 0.875rem;
  background: #1a1d27;
  border-bottom: 1px solid #2d3248;
  font-size: 0.75rem;
  text-transform: uppercase;
  letter-spacing: 0.08em;
  color: #64748b;
  font-weight: 600;
}

.discovery-server {
  padding: 0.5rem 0.875rem;
  border-bottom: 1px solid #1e2235;
  &:last-child { border-bottom: none; }
}

.server-name {
  font-size: 0.875rem;
  color: #94a3b8;
  font-weight: 500;
  margin-bottom: 0.375rem;
  display: flex;
  align-items: center;
  gap: 0.5rem;
}

.server-hostname {
  font-size: 0.8125rem;
  color: #64748b;
}

.server-uuid {
  font-size: 0.75rem;
  background: #1a1d27;
  padding: 0.1em 0.35em;
  border-radius: 3px;
  color: #64748b;
}

.no-server-ports {
  margin: 0;
  font-size: 0.8125rem;
  color: #475569;
}

.discovery-ports {
  display: flex;
  flex-direction: column;
  gap: 0.25rem;
}

.discovery-port-row {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  font-size: 0.8125rem;
  padding: 0.1875rem 0;

  &.is-disabled { opacity: 0.5; }
}

.discovery-check { cursor: pointer; }

.dp-ports {
  font-size: 0.8125rem;
  background: #1a1d27;
  padding: 0.1em 0.4em;
  border-radius: 3px;
  color: #7dd3fc;
}

.dp-name {
  color: #94a3b8;
  flex: 1;
}

.dp-badge {
  font-size: 0.6875rem;
  padding: 0.1em 0.45em;
  border-radius: 4px;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.04em;

  &.auto     { background: rgba(79,110,247,.15); color: #818cf8; }
  &.enabled  { background: rgba(52,211,153,.15); color: #6ee7b7; }
  &.disabled { background: rgba(100,116,139,.1); color: #64748b; }
}

.btn-icon {
  background: none;
  border: none;
  cursor: pointer;
  padding: 0.125rem 0.3rem;
  border-radius: 3px;
  color: #64748b;
  font-size: 0.8125rem;
  line-height: 1;

  &:hover { color: #94a3b8; background: rgba(255,255,255,.06); }
}

.btn-pin:hover { color: #fbbf24; }
.btn-reset:hover { color: #f87171; background: rgba(239,68,68,.1); }

.btn-pin-ok {
  padding: 0.2rem 0.5rem;
  background: #4f6ef7;
  border: none;
  border-radius: 4px;
  color: #fff;
  font-size: 0.8125rem;
  cursor: pointer;
  &:hover { background: #3d5ce5; }
}

.pin-input {
  width: 80px;
  padding: 0.2rem 0.375rem;
  background: #0f1117;
  border: 1px solid #4f6ef7;
  border-radius: 4px;
  color: #e2e8f0;
  font-size: 0.8125rem;
  &:focus { outline: none; }
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
  &:hover { color: #7dd3fc; border-bottom-style: solid; }
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

.cmd-flag-discovery {
  color: #86efac;
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
