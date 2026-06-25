<script setup lang="ts">
defineProps<{
  entity: { id: string; name: string | null }
}>()
</script>

<template>
  <span v-if="entity.name">{{ entity.name }}</span>
  <code v-else>{{ entity.id }}</code>
</template>
