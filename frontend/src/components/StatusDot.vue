<script setup lang="ts">
import { computed } from 'vue'
import { dotEmoji, dotLabel, ringLabel, selfLabel, type RemoteStatus } from '@/liveStatus'

const props = defineProps<{
  live: boolean
  remoteStatus?: RemoteStatus | null
  /**
   * This side's own 4-state status, fills the dot itself instead of the
   * plain live/not-live emoji — e.g. a service row's own forwarding state.
   * Omit to keep the old boolean-live dot (e.g. a subscription row, whose
   * own state really is just "am I bridging right now or not").
   */
  selfStatus?: RemoteStatus | null
  connectionCount?: number
}>()

const emoji = computed(() => (props.selfStatus ? null : dotEmoji(props.live)))
const label = computed(() => {
  const parts = [props.selfStatus ? selfLabel[props.selfStatus] : dotLabel(props.live)]
  if (props.remoteStatus) parts.push(ringLabel[props.remoteStatus])
  if (props.connectionCount) parts.push(`${props.connectionCount} connected`)
  return parts.join(' · ')
})
const selfClass = computed(() => (props.selfStatus ? `self-${props.selfStatus}` : (props.live ? 'live' : 'not-live')))
const ringClass = computed(() => (props.remoteStatus ? `ring-${props.remoteStatus}` : null))
const badgeText = computed(() => (props.connectionCount && props.connectionCount > 0
  ? (props.connectionCount > 9 ? '9+' : String(props.connectionCount))
  : null))
</script>

<template>
  <span
    class="status-dot"
    :class="[selfClass, ringClass]"
    :title="label"
    role="img"
    :aria-label="label"
    >{{ emoji }}<span v-if="badgeText" class="status-badge">{{ badgeText }}</span></span
  >
</template>

<style lang="scss" scoped>
.status-dot {
  position: relative;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 1.1875rem;
  height: 1.1875rem;
  font-size: 0.8125rem;
  line-height: 1;
  border-radius: 50%;
  box-shadow: 0 0 0 2px transparent;

  &.ring-offline {
    box-shadow: 0 0 0 2px #9e9e9e;
  }

  &.ring-not_forwarded {
    box-shadow: 0 0 0 2px #ff9800;
  }

  &.ring-idle {
    box-shadow: 0 0 0 2px #2196f3;
  }

  &.ring-active {
    box-shadow: 0 0 0 2px #4caf50;
  }

  &.self-offline {
    background: #9e9e9e;
  }

  &.self-not_forwarded {
    background: #ff9800;
  }

  &.self-idle {
    background: #2196f3;
  }

  &.self-active {
    background: #4caf50;
  }
}

.status-badge {
  position: absolute;
  top: -0.375rem;
  right: -0.375rem;
  min-width: 0.9375rem;
  height: 0.9375rem;
  padding: 0 0.1875rem;
  border-radius: 999px;
  background: #ef4444;
  color: #fff;
  font-size: 0.5625rem;
  font-weight: 700;
  line-height: 0.9375rem;
  text-align: center;
  box-shadow: 0 0 0 2px #12141f;
}
</style>
