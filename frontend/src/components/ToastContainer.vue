<script setup lang="ts">
import { useToast } from '@/composables/useToast'
const { toasts, dismiss } = useToast()
</script>

<template>
  <Teleport to="body">
    <div class="toast-container">
      <div
        v-for="t in toasts"
        :key="t.id"
        class="toast"
        :class="t.level"
        @click="dismiss(t.id)"
      >
        {{ t.message }}
      </div>
    </div>
  </Teleport>
</template>

<style lang="scss" scoped>
.toast-container {
  position: fixed;
  bottom: 1.5rem;
  right: 1.5rem;
  z-index: 9999;
  display: flex;
  flex-direction: column;
  gap: 0.5rem;
  max-width: 28rem;
}

.toast {
  padding: 0.75rem 1rem;
  border-radius: 6px;
  font-size: 0.875rem;
  line-height: 1.4;
  cursor: pointer;
  animation: slide-in 0.15s ease;

  &.error {
    background: #2d1a1a;
    border: 1px solid #7f1d1d;
    color: #fca5a5;
  }

  &.info {
    background: #1a1d2d;
    border: 1px solid #1e3a5f;
    color: #93c5fd;
  }

  &.success {
    background: #0f2a1a;
    border: 1px solid #14532d;
    color: #6ee7b7;
  }

  &:hover { filter: brightness(1.1); }
}

@keyframes slide-in {
  from { opacity: 0; transform: translateX(1rem); }
  to   { opacity: 1; transform: translateX(0); }
}
</style>
