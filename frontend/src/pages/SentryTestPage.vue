<script setup lang="ts">
import { onMounted } from 'vue'
import AppShell from '@/components/AppShell.vue'
import { adminApi } from '@/api/admin'

onMounted(async () => {
  try {
    await adminApi.sampleError()
  } catch {
    // expected — the backend route deliberately panics
  }
  setTimeout(() => {
    throw new Error('frontend sample error for Sentry/Bugsink verification')
  }, 0)
})
</script>

<template>
  <AppShell>
    <h1>Sentry/Bugsink verification</h1>
    <p>
      Triggered a backend sample error (<code>/api/admin/sample-error</code>) and a frontend
      sample error. Check Bugsink for both events.
    </p>
  </AppShell>
</template>
