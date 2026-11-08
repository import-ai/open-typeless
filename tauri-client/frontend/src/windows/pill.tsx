import { useEffect, useState } from 'react'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { emitTo } from '@tauri-apps/api/event'
import { RecordingPill } from '@/components/recording-pill'
import { useTauriEvent } from '@/hooks/use-tauri-event'
import { useWaveform } from '@/hooks/use-waveform'
import { commands, desktop, previewPillError, type MicState, type PillError, type PillState } from '@/lib/desktop'

export function PillWindow() {
  const [state, setState] = useState<PillState>('unready')
  const [preview, setPreview] = useState(false)
  const [error, setError] = useState(previewPillError)
  const { levels, showLevels } = useWaveform(state === 'ready', preview)
  useTauriEvent<PillState>('pill-preview', (next) => { setPreview(true); setState(next); setError(previewPillError) })
  useTauriEvent('pill-hidden', () => { setPreview(false); setState('disconnected') })
  useTauriEvent<MicState>('mic-state', next => setState(current => current === 'error' || current === 'processing' ? current : next))
  useTauriEvent<PillError>('recording-error', next => { setPreview(false); setError(next); setState('error') })
  useTauriEvent<readonly number[]>('mic-waveform', showLevels)
  useTauriEvent('recording-starting', () => { setPreview(false); setState('unready') })
  useTauriEvent('recording-processing', () => setState('processing'))
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
    <RecordingPill onDrag={drag} state={state} error={error} levels={levels} onCancel={() => request('cancel-requested')} onDone={() => request('stop-requested')} />
  </div>
}
