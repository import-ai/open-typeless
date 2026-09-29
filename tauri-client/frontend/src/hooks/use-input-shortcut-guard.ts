import { useEffect, useRef, useState } from 'react'
import { commands, desktop } from '@/lib/desktop'

// Serialize suspend/resume calls so a quickly closed editor cannot leave the
// native shortcut disabled. Window blur also restores it on the Rust side.
export function useInputShortcutGuard(active: boolean, onChange: (active: boolean) => void) {
  const queue = useRef(Promise.resolve())
  const [ready, setReady] = useState(false)
  const [error, setError] = useState('')
  useEffect(() => {
    let disposed = false
    const apply = (suspend: boolean) => {
      onChange(suspend)
      setReady(false)
      queue.current = queue.current.catch(() => {}).then(async () => {
        if (desktop) await commands.captureShortcut(suspend)
      }).then(() => { if (!disposed) { setReady(suspend); setError('') } })
        .catch(error => { if (!disposed) setError(`无法切换语音快捷键: ${String(error)}`) })
    }
    apply(active && document.hasFocus())
    const focus = () => apply(active)
    const blur = () => apply(false)
    window.addEventListener('focus', focus)
    window.addEventListener('blur', blur)
    return () => {
      disposed = true
      window.removeEventListener('focus', focus)
      window.removeEventListener('blur', blur)
      onChange(false)
      queue.current = queue.current.catch(() => {}).then(async () => {
        if (desktop) await commands.captureShortcut(false)
      }).catch(console.error)
    }
  }, [active, onChange])
  return { ready: !desktop || ready, error }
}
