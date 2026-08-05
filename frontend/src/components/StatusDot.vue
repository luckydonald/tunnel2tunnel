<script setup lang="ts">
import { computed } from 'vue'
import { dotEmoji, dotLabel, ringLabel, type RemoteStatus } from '@/liveStatus'

const props = defineProps<{
  live: boolean
  remoteStatus?: RemoteStatus | null
  connectionCount?: number
}>()

const emoji = computed(() => dotEmoji(props.live))
const label = computed(() => {
  const parts = [dotLabel(props.live)]
  if (props.remoteStatus) parts.push(ringLabel[props.remoteStatus])
  if (props.connectionCount) parts.push(`${props.connectionCount} connected`)
  return parts.join(' · ')
})
const ringClass = computed(() => (props.remoteStatus ? `ring-${props.remoteStatus}` : null))
const badgeText = computed(() => (props.connectionCount && props.connectionCount > 0
  ? (props.connectionCount > 9 ? '9+' : String(props.connectionCount))
  : null))
</script>

<template>
  <span
    class="status-dot"
    :class="[live ? 'live' : 'not-live', ringClass]"
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
