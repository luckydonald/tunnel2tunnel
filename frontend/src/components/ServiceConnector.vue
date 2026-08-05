<script setup lang="ts">
import { ref, computed } from 'vue'
import { RouterLink } from 'vue-router'
import type { SubscribableOwner, SubscribableService } from '@/api/entities'
import type { SubscriptionLiveStatus } from '@/api/liveConnections'
import { roleBadges } from '@/labels'
import StatusDot from '@/components/StatusDot.vue'

const props = defineProps<{
  subscribableOwners: SubscribableOwner[]
  /** The current user's own entity IDs, used for the Mine/Friends Origin filter. */
  ownEntityIds: string[]
  /** Live status per subscription_id, keyed the same way as the entity-detail page's map. Optional — omitted in contexts without live data. */
  subscriptionStatus?: Map<string, SubscriptionLiveStatus>
}>()

const emit = defineEmits<{
  subscribe: [portConfigId: string, localPort: number]
  unsubscribe: [subscriptionId: string]
  'update-local-port': [subscriptionId: string, localPort: number]
}>()

type ShowFilter = 'configured' | 'unconfigured' | 'all'
const showFilter = ref<ShowFilter>('configured')

type OriginFilter = 'mine' | 'friends' | 'all'
const originFilter = ref<OriginFilter>('all')

const ownIds = computed(() => new Set(props.ownEntityIds))

function isOwn(ownerId: string): boolean {
  return ownIds.value.has(ownerId)
}

// Each owner filtered down to just the services relevant to the current Show filter.
interface FilteredOwner extends SubscribableOwner {
  visibleServices: SubscribableService[]
}

const filteredOwners = computed((): FilteredOwner[] => {
  const originOk = (ownerId: string): boolean => {
    if (showFilter.value === 'configured') return true
    if (originFilter.value === 'all') return true
    if (originFilter.value === 'mine') return isOwn(ownerId)
    return !isOwn(ownerId)
  }

  const result: FilteredOwner[] = []
  for (const owner of props.subscribableOwners) {
    if (!originOk(owner.id)) continue
    const visibleServices = owner.services.filter(s => {
      if (showFilter.value === 'configured') return s.subscription !== null
      if (showFilter.value === 'unconfigured') return s.subscription === null
      return true
    })
    if (visibleServices.length) {
      result.push({ ...owner, visibleServices })
    }
  }
  return result
})

// Inline local-port editing state (per subscription)
const editingSubscriptionId = ref<string | null>(null)
const editLocalPort = ref<number>(0)

function startEditLocalPort(subscriptionId: string, currentPort: number): void {
  editingSubscriptionId.value = subscriptionId
  editLocalPort.value = currentPort
}

function confirmEditLocalPort(subscriptionId: string): void {
  emit('update-local-port', subscriptionId, editLocalPort.value)
  editingSubscriptionId.value = null
}

function cancelEditLocalPort(): void {
  editingSubscriptionId.value = null
}

// New-subscription local-port input state (per not-yet-subscribed service)
const subscribeLocalPort = ref<Record<string, number>>({})

function localPortFor(service: SubscribableService): number {
  return subscribeLocalPort.value[service.id] ?? service.proxy_port
}

function setLocalPortFor(service: SubscribableService, value: number): void {
  subscribeLocalPort.value[service.id] = value
}

function handleSubscribe(service: SubscribableService): void {
  emit('subscribe', service.id, localPortFor(service))
}

function statusFor(subscriptionId: string): SubscriptionLiveStatus | undefined {
  return props.subscriptionStatus?.get(subscriptionId)
}
</script>

<template>
  <div class="service-connector">
    <div class="filter-row">
      <span class="filter-label">Show:</span>
      <button
        v-for="opt in (['configured', 'unconfigured', 'all'] as ShowFilter[])"
        :key="opt"
        type="button"
        class="filter-btn"
        :class="{ active: showFilter === opt }"
        @click="showFilter = opt"
      >{{ opt === 'configured' ? 'Configured' : opt === 'unconfigured' ? 'Unconfigured' : 'All' }}</button>

      <template v-if="showFilter !== 'configured'">
        <span class="filter-label filter-label-origin">Origin:</span>
        <button
          v-for="opt in (['mine', 'friends', 'all'] as OriginFilter[])"
          :key="opt"
          type="button"
          class="filter-btn"
          :class="{ active: originFilter === opt }"
          @click="originFilter = opt"
        >{{ opt === 'mine' ? 'Mine' : opt === 'friends' ? 'Friends' : 'All' }}</button>
      </template>
    </div>

    <div v-if="filteredOwners.length" class="owners">
      <div v-for="owner in filteredOwners" :key="owner.id" class="owner-group">
        <div class="owner-name">
          <RouterLink :to="'/entities/' + owner.id" class="entity-link">
            {{ owner.name ?? owner.id.slice(0, 13) + '…' }}
          </RouterLink>
          <span class="owner-badges">{{ roleBadges(owner) }}</span>
          <span v-if="!owner.online" class="owner-offline">offline</span>
        </div>

        <template v-for="service in owner.visibleServices" :key="service.id">
          <div class="service-row">
            <StatusDot
              v-if="service.subscription && statusFor(service.subscription.id)"
              :live="statusFor(service.subscription.id)!.live"
              :remote-status="statusFor(service.subscription.id)!.remote_status"
            />
            <span class="service-name">{{ service.name }}</span>
            <code class="service-port">proxy {{ service.proxy_port }}</code>

            <template v-if="service.subscription">
              <template v-if="editingSubscriptionId === service.subscription.id">
                <input
                  v-model.number="editLocalPort"
                  type="number"
                  class="local-port-input"
                  min="1"
                  max="65535"
                  @keyup.enter="confirmEditLocalPort(service.subscription.id)"
                  @keyup.esc="cancelEditLocalPort"
                />
                <button class="btn-sm btn-primary" @click="confirmEditLocalPort(service.subscription.id)">Save</button>
                <button class="btn-sm btn-secondary" @click="cancelEditLocalPort">Cancel</button>
              </template>
              <template v-else>
                <span class="local-port-arrow">→ my local port</span>
                <code
                  class="service-port editable"
                  title="Click to edit"
                  @click="startEditLocalPort(service.subscription.id, service.subscription.subscriber_local_port)"
                >{{ service.subscription.subscriber_local_port }}</code>
                <button class="btn-sm btn-danger" @click="emit('unsubscribe', service.subscription.id)">Unsubscribe</button>
              </template>
            </template>
            <template v-else>
              <span class="local-port-arrow">→</span>
              <input
                :value="localPortFor(service)"
                type="number"
                class="local-port-input"
                min="1"
                max="65535"
                @input="setLocalPortFor(service, +($event.target as HTMLInputElement).value)"
              />
              <button class="btn-sm btn-primary" @click="handleSubscribe(service)">Subscribe</button>
            </template>
          </div>
          <p
            v-if="service.subscription && statusFor(service.subscription.id)?.remote_status === 'not_forwarded'"
            class="orange-hint"
          >server is online but hasn't forwarded this port yet — will connect automatically once it does</p>
        </template>
      </div>
    </div>
    <p v-else class="empty">
      {{ showFilter === 'configured' ? 'No subscriptions yet — switch to "Unconfigured" or "All" to browse available services.' : 'Nothing to show.' }}
    </p>
  </div>
</template>

<style lang="scss" scoped>
.service-connector {
  display: flex;
  flex-direction: column;
  gap: 1rem;
}

.filter-row {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  flex-wrap: wrap;
  font-size: 0.875rem;
}

.filter-label {
  color: #94a3b8;
  &.filter-label-origin { margin-left: 1rem; }
}

.filter-btn {
  padding: 0.25rem 0.625rem;
  background: none;
  border: 1px solid #2d3248;
  border-radius: 4px;
  color: #94a3b8;
  font-size: 0.8125rem;
  cursor: pointer;

  &:hover { color: #e2e8f0; border-color: #4f6ef7; }
  &.active { background: rgba(79, 110, 247, 0.15); border-color: #4f6ef7; color: #93c5fd; }
}

.owners {
  display: flex;
  flex-direction: column;
  gap: 1rem;
}

.owner-group {
  border: 1px solid #2d3248;
  border-radius: 6px;
  padding: 0.75rem 1rem;
}

.owner-name {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  margin-bottom: 0.5rem;
  font-size: 0.9375rem;
}

.owner-badges { font-size: 0.8125rem; color: #94a3b8; }
.owner-offline {
  font-size: 0.75rem;
  color: #64748b;
  text-transform: uppercase;
  letter-spacing: 0.05em;
}

.entity-link { color: #7dd3fc; text-decoration: none; font-weight: 500; &:hover { text-decoration: underline; } }

.service-row {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  padding: 0.25rem 0;
  font-size: 0.875rem;
}

.service-name { color: #e2e8f0; min-width: 100px; }

.service-port {
  background: #1a1d27;
  padding: 0.1em 0.4em;
  border-radius: 3px;
  font-size: 0.8125rem;
  color: #7dd3fc;

  &.editable { cursor: pointer; &:hover { background: #2d3248; } }
}

.local-port-arrow { color: #64748b; font-size: 0.8125rem; }

.local-port-input {
  width: 80px;
  padding: 0.2rem 0.4rem;
  background: #0f1117;
  border: 1px solid #2d3248;
  border-radius: 4px;
  color: #e2e8f0;
  font-size: 0.8125rem;
  &:focus { outline: none; border-color: #4f6ef7; }
}

.btn-sm {
  padding: 0.2rem 0.625rem;
  border-radius: 4px;
  font-size: 0.8125rem;
  cursor: pointer;
  border: 1px solid transparent;
}

.btn-primary { background: #4f6ef7; border-color: #4f6ef7; color: #fff; &:hover { background: #3d5ce5; } }
.btn-secondary { background: none; border-color: #2d3248; color: #94a3b8; &:hover { color: #e2e8f0; border-color: #4f6ef7; } }
.btn-danger { background: none; border-color: #3f1e1e; color: #f87171; &:hover { background: rgba(239,68,68,.1); } }

.empty { color: #64748b; font-size: 0.875rem; }

.orange-hint {
  margin: -0.25rem 0 0.25rem 1.5rem;
  font-size: 0.75rem;
  color: #fb923c;
}
</style>
