import { RecordingPill } from '@/components/recording-pill'
import { useWaveform } from '@/hooks/use-waveform'
import type { PillState } from '@/lib/desktop'

const states: { state: PillState; label: string }[] = [
  { state: 'disconnected', label: '未连接' },
  { state: 'unready', label: '未就绪' },
  { state: 'ready', label: '已就绪' },
  { state: 'processing', label: '识别中' },
  { state: 'error', label: '报错' },
]
export function PillDebug() {
  const { levels } = useWaveform(true, true)
  return <main className="pill-debug-gallery" aria-label="Pill 状态预览">
    {states.map(({ state, label }) => <div className="pill-debug-row" key={state}>
      <p className="pill-debug-label">{label}</p>
      <div className="pill-stage"><RecordingPill state={state} levels={levels} /></div>
    </div>)}

  </main>
}
