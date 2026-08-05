const INITIAL_BACKOFF_MS = 1000
const MAX_BACKOFF_MS = 15000

/**
 * Connects a WebSocket to `path` (same-origin, cookies ride along
 * automatically) and calls `onMessage` with each JSON message the server
 * pushes. Reconnects with capped exponential backoff on close/error.
 *
 * Plain function, not a composable — manual lifecycle (`close()`), not tied
 * to a component's `onMounted`/`onUnmounted`, so callers that need a
 * connection to outlive any single component (e.g. a Pinia store) can use
 * it directly. Also bidirectional (`send()`), unlike the old
 * `useLiveSocket` composable this replaces.
 */
export function createLiveSocket<T>(
  path: string,
  onMessage: (data: T) => void,
): { close: () => void; send: (data: unknown) => void } {
  let socket: WebSocket | null = null
  let backoffMs = INITIAL_BACKOFF_MS
  let reconnectTimer: ReturnType<typeof setTimeout> | null = null
  let stopped = false

  function connect(): void {
    if (stopped) return
    const url = (location.protocol === 'https:' ? 'wss://' : 'ws://') + location.host + path
    console.debug(`[createLiveSocket] connecting: ${path}`)
    socket = new WebSocket(url)

    socket.addEventListener('open', () => {
      console.debug(`[createLiveSocket] connected: ${path}`)
      backoffMs = INITIAL_BACKOFF_MS
    })

    socket.addEventListener('message', event => {
      try {
        const data = JSON.parse(event.data as string) as T
        console.debug(`[createLiveSocket] message: ${path}`, data)
        onMessage(data)
      } catch (e) {
        console.warn(`[createLiveSocket] failed to parse message: ${path}`, e)
      }
    })

    socket.addEventListener('close', () => {
      console.warn(`[createLiveSocket] closed, reconnecting in ${backoffMs}ms: ${path}`)
      scheduleReconnect()
    })

    socket.addEventListener('error', e => {
      console.warn(`[createLiveSocket] error: ${path}`, e)
    })
  }

  function scheduleReconnect(): void {
    if (stopped) return
    reconnectTimer = setTimeout(() => {
      backoffMs = Math.min(backoffMs * 2, MAX_BACKOFF_MS)
      connect()
    }, backoffMs)
  }

  connect()

  return {
    close(): void {
      stopped = true
      if (reconnectTimer) clearTimeout(reconnectTimer)
      socket?.close()
    },
    send(data: unknown): void {
      if (socket?.readyState === WebSocket.OPEN) {
        socket.send(JSON.stringify(data))
      } else {
        console.warn(`[createLiveSocket] send while not open, dropped: ${path}`, data)
      }
    },
  }
}
