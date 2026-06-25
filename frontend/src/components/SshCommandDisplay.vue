<script setup lang="ts">
import { ref, computed } from 'vue'
import type { Entity, EntityPort } from '@/api/entities'

const props = withDefaults(defineProps<{
  entity: Entity
  ports: EntityPort[]
  t2tHost?: string
  t2tSshPort?: number
}>(), {
  t2tSshPort: 2222,
})

const hoveredPortId = ref<string | null>(null)
const t2tHost = computed(() => props.t2tHost ?? window.location.hostname)
const enabledPorts = computed(() => props.ports.filter(p => p.enabled))

function portFlag(port: EntityPort): string {
  return props.entity.entity_type === 'server'
    ? `-R ${port.proxy_port}:localhost:${port.local_port}`
    : `-L ${port.local_port}:<server>:${port.proxy_port}`
}

function isHovered(portId: string): boolean {
  return hoveredPortId.value === portId
}
</script>

<template>
  <div class="ssh-cmd-display">
    <div class="cmd-block">
      <div class="cmd-header">
        <span class="cmd-label">SSH command</span>
        <span class="cmd-hint" v-if="entity.entity_type === 'client'">
          Replace <code>&lt;server&gt;</code> with the server entity ID
        </span>
      </div>
      <pre class="cmd-text">ssh -N<template v-for="port in enabledPorts" :key="port.id"> \
  <span
    class="cmd-flag"
    :class="{ 'is-hovered': isHovered(port.id) }"
    @mouseenter="hoveredPortId = port.id"
    @mouseleave="hoveredPortId = null"
  >{{ portFlag(port) }}</span></template> \
  {{ entity.id }}@{{ t2tHost }} \
  -p {{ t2tSshPort }}</pre>
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

.cmd-flag {
  color: #7dd3fc;
  border-radius: 2px;
  padding: 0.1em 0;
  cursor: default;
  transition: background 0.1s;

  &.is-hovered {
    background: rgba(79, 110, 247, 0.25);
    color: #93c5fd;
  }
}

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
