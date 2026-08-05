<script setup lang="ts">
import { watch } from 'vue'
import { RouterView, useRoute } from 'vue-router'
import ToastContainer from '@/components/ToastContainer.vue'
import { useAuthStore } from '@/stores/auth'
import { useLiveConnectionsStore } from '@/stores/liveConnections'

const route = useRoute()
const auth = useAuthStore()
const liveConnections = useLiveConnectionsStore()

// `App.vue` is never remounted (unlike routed page components, which
// `:key="route.path"` below forces to remount on every navigation) — the one
// place a connection meant to survive navigation can live. `immediate: true`
// also covers a page reload while already logged in: `user` starts `null`
// until the existing bootstrap `fetchMe()` resolves from the still-valid
// session cookie, and the watcher needs to react to that transition (or, in
// the unlikely case `user` is already populated by the time this registers,
// fire without depending on a change at all).
watch(
  () => auth.user,
  user => {
    if (user) liveConnections.connect()
    else liveConnections.disconnect()
  },
  { immediate: true },
)
</script>

<template>
  <RouterView :key="route.path" />
  <ToastContainer />
</template>
