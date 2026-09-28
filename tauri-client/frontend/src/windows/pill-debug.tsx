import { useEffect, useState } from 'react'
import { RecordingPill } from '@/components/recording-pill'
import type { PillState } from '@/lib/desktop'

const states: { state: PillState; label: string }[] = [
  { state: 'disconnected', label: '未连接' },
  { state: 'unready', label: '未就绪' },
  { state: 'ready', label: '已就绪' },
  { state: 'processing', label: '识别中' },
]
export function PillDebug() {
  const [level, setLevel] = useState(0.06)
  useEffect(() => {
    const timer = setInterval(() => setLevel(0.04 + Math.random() * 0.04), 180)
    return () => clearInterval(timer)
  }, [])
  return <main className="pill-debug-gallery" aria-label="Pill 状态预览">
    {states.map(({ state, label }) => <div className="pill-debug-row" key={state}>
      <p className="pill-debug-label">{label}</p>
      <div className="pill-stage"><RecordingPill state={state} level={level} /></div>
    </div>)}
  </main>
}
