import { useEffect, useState } from 'react'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { emitTo } from '@tauri-apps/api/event'
import { RecordingPill } from '@/components/recording-pill'
import { useTauriEvent } from '@/hooks/use-tauri-event'
import { commands, desktop, type MicState, type PillState } from '@/lib/desktop'

export function PillWindow() {
  const [state, setState] = useState<PillState>('unready')
  const [level, setLevel] = useState(0)
  const [preview, setPreview] = useState(false)
  useTauriEvent<PillState>('pill-preview', (next) => { setPreview(true); setState(next); setLevel(0.06) })
  useEffect(() => {
    if (!preview || state !== 'ready') return
    const timer = setInterval(() => setLevel(0.04 + Math.random() * 0.04), 180)
    return () => clearInterval(timer)
  }, [preview, state])
  useTauriEvent('pill-hidden', () => { setPreview(false); setState('disconnected'); setLevel(0) })
  useTauriEvent<MicState>('mic-state', setState)
  useTauriEvent<number>('mic-level', setLevel)
  useTauriEvent('recording-starting', () => { setPreview(false); setState('unready'); setLevel(0) })
  useTauriEvent('recording-processing', () => setState('processing'))
  useTauriEvent('recording-stopped', () => setState('processing'))
  useTauriEvent('recording-cancelled', () => setState('disconnected'))
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') { event.preventDefault(); void commands.cancel().catch(console.error) }
    }
    document.addEventListener('keydown', onKeyDown)
    return () => document.removeEventListener('keydown', onKeyDown)
  }, [])
  const drag = () => {
    if (desktop) void getCurrentWindow().startDragging().catch(console.error)
  }
  const request = (name: string) => {
    if (preview) {
      if (name === 'cancel-requested') void commands.cancel().catch(console.error)
      else setState('processing')
      return
    }
    void emitTo('main', name).catch((error: unknown) => console.error(error))
  }
  return <div className="pill-stage">
    <RecordingPill onDrag={drag} state={state} level={level} onCancel={() => request('cancel-requested')} onDone={() => request('stop-requested')} />
  </div>
}
