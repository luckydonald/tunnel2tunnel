<script setup lang="ts">
import { computed } from 'vue'
import { statusDotEmoji, statusDotLabel, type LiveStatus } from '@/liveStatus'

const props = defineProps<{
  status: LiveStatus
}>()

const emoji = computed(() => statusDotEmoji[props.status])
const label = computed(() => statusDotLabel[props.status])
</script>

<template>
  <span class="status-dot" :class="status" :title="label" role="img" :aria-label="label">{{ emoji }}</span>
</template>

<style lang="scss" scoped>
.status-dot {
  display: inline-block;
  font-size: 0.8125rem;
  line-height: 1;
}
</style>
