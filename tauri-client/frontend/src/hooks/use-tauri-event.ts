import { useEffect, useLayoutEffect, useRef } from 'react'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { desktop } from '@/lib/desktop'

// Async subscriptions must also be disposed if StrictMode/HMR unmounts before
// listen() resolves. Keep handlers current without re-subscribing on every frame.
export function useTauriEvent<T>(name: string, handler: (payload: T) => void) {
  const latest = useRef(handler)
  useLayoutEffect(() => { latest.current = handler }, [handler])
  useEffect(() => {
    if (!desktop) return
    let disposed = false
    let unlisten: UnlistenFn | undefined
    void listen<T>(name, (event) => {
      if (!disposed) latest.current(event.payload)
    }).then((cleanup) => {
      if (disposed) cleanup()
      else unlisten = cleanup
    }).catch((error: unknown) => console.error(`Cannot listen to ${name}`, error))
    return () => { disposed = true; unlisten?.() }
  }, [name])
}
