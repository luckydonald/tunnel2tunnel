<script setup lang="ts">
import AppShell from '@/components/AppShell.vue'
import { useAuthStore } from '@/stores/auth'

const auth = useAuthStore()
</script>

<template>
  <AppShell>
    <h1>Dashboard</h1>
    <p>Welcome, <strong>{{ auth.user?.username }}</strong>.</p>
  </AppShell>
</template>
