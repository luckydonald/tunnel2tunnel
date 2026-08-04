import { onMounted, onUnmounted } from 'vue'

const INITIAL_BACKOFF_MS = 1000
const MAX_BACKOFF_MS = 15000

/**
 * Connects a WebSocket to `path` (same-origin, cookies ride along
 * automatically) and calls `onMessage` with each JSON message the server
 * pushes. Reconnects with capped exponential backoff on close/error.
 * Cleans up on unmount.
 */
export function useLiveSocket<T>(path: string, onMessage: (data: T) => void): void {
  let socket: WebSocket | null = null
  let backoffMs = INITIAL_BACKOFF_MS
  let reconnectTimer: ReturnType<typeof setTimeout> | null = null
  let stopped = false

  function connect(): void {
    if (stopped) return
    const url = (location.protocol === 'https:' ? 'wss://' : 'ws://') + location.host + path
    console.debug(`[useLiveSocket] connecting: ${path}`)
    socket = new WebSocket(url)

    socket.addEventListener('open', () => {
      console.debug(`[useLiveSocket] connected: ${path}`)
      backoffMs = INITIAL_BACKOFF_MS
    })

    socket.addEventListener('message', event => {
      try {
        const data = JSON.parse(event.data as string) as T
        console.debug(`[useLiveSocket] message: ${path}`, data)
        onMessage(data)
      } catch (e) {
        console.warn(`[useLiveSocket] failed to parse message: ${path}`, e)
      }
    })

    socket.addEventListener('close', () => {
      console.warn(`[useLiveSocket] closed, reconnecting in ${backoffMs}ms: ${path}`)
      scheduleReconnect()
    })

    socket.addEventListener('error', e => {
      console.warn(`[useLiveSocket] error: ${path}`, e)
    })
  }

  function scheduleReconnect(): void {
    if (stopped) return
    reconnectTimer = setTimeout(() => {
      backoffMs = Math.min(backoffMs * 2, MAX_BACKOFF_MS)
      connect()
    }, backoffMs)
  }

  onMounted(connect)

  onUnmounted(() => {
    stopped = true
    if (reconnectTimer) clearTimeout(reconnectTimer)
    socket?.close()
  })
}
