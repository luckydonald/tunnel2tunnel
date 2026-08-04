<script setup lang="ts">
import { computed } from 'vue'
import { dotEmoji, dotLabel, ringLabel, type RemoteStatus } from '@/liveStatus'

const props = defineProps<{
  live: boolean
  remoteStatus?: RemoteStatus | null
}>()

const emoji = computed(() => dotEmoji(props.live))
const label = computed(() => {
  const parts = [dotLabel(props.live)]
  if (props.remoteStatus) parts.push(ringLabel[props.remoteStatus])
  return parts.join(' · ')
})
const ringClass = computed(() => (props.remoteStatus ? `ring-${props.remoteStatus}` : null))
</script>

<template>
  <span
    class="status-dot"
    :class="[live ? 'live' : 'not-live', ringClass]"
    :title="label"
    role="img"
    :aria-label="label"
    >{{ emoji }}</span
  >
</template>

<style lang="scss" scoped>
.status-dot {
  display: inline-block;
  font-size: 0.8125rem;
  line-height: 1;
  padding: 0.1875rem;
  border-radius: 50%;
  box-shadow: 0 0 0 2px transparent;

  &.ring-gray {
    box-shadow: 0 0 0 2px #9e9e9e;
  }

  &.ring-orange {
    box-shadow: 0 0 0 2px #ff9800;
  }

  &.ring-green {
    box-shadow: 0 0 0 2px #4caf50;
  }
}
</style>
