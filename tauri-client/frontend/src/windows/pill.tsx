import { useState } from 'react'
import { emitTo } from '@tauri-apps/api/event'
import { RecordingPill } from '@/components/recording-pill'
import { useTauriEvent } from '@/hooks/use-tauri-event'
import type { MicState, PillState } from '@/lib/desktop'

export function PillWindow() {
  const [state, setState] = useState<PillState>('unready')
  const [level, setLevel] = useState(0)
  useTauriEvent<MicState>('mic-state', setState)
  useTauriEvent<number>('mic-level', setLevel)
  useTauriEvent('recording-starting', () => { setState('unready'); setLevel(0) })
  useTauriEvent('recording-processing', () => setState('processing'))
  useTauriEvent('recording-stopped', () => setState('processing'))
  useTauriEvent('recording-cancelled', () => setState('disconnected'))
  const request = (name: string) => {
    void emitTo('main', name).catch((error: unknown) => console.error(error))
  }
  return <div className="pill-stage">
    <RecordingPill state={state} level={level} onCancel={() => request('cancel-requested')} onDone={() => request('stop-requested')} />
  </div>
}
