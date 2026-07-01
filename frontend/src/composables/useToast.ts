import { ref } from 'vue'

export type ToastLevel = 'error' | 'info' | 'success'

interface Toast {
  id: number
  message: string
  level: ToastLevel
}

const toasts = ref<Toast[]>([])
let nextId = 1

export function useToast() {
  function show(message: string, level: ToastLevel = 'error', durationMs = 6000): void {
    const id = nextId++
    toasts.value.push({ id, message, level })
    setTimeout(() => {
      toasts.value = toasts.value.filter(t => t.id !== id)
    }, durationMs)
  }

  function dismiss(id: number): void {
    toasts.value = toasts.value.filter(t => t.id !== id)
  }

  return { toasts, show, dismiss }
}
