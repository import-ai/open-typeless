import { useState } from 'react'
import { commands, desktop, type PillState } from '@/lib/desktop'

type PreviewState = PillState | 'hidden'
const states: { value: PreviewState; label: string }[] = [
  { value: 'hidden', label: '隐藏' },
  { value: 'disconnected', label: '未连接' },
  { value: 'unready', label: '未就绪' },
  { value: 'ready', label: '已就绪' },
  { value: 'processing', label: '识别中' },
]

export function DeveloperOptions({ disabled }: { disabled: boolean }) {
  const [state, setState] = useState<PreviewState>('hidden')
  const [busy, setBusy] = useState(false)
  const [status, setStatus] = useState('不录音、不调用 ASR，直接预览桌面 pill。')

  async function preview(mode: PreviewState) {
    setBusy(true)
    try {
      await commands.previewPill(mode)
      setState(mode)
      setStatus(mode === 'hidden' ? 'Pill 已隐藏' : `正在展示：${states.find(item => item.value === mode)?.label}`)
    } catch (error) { setStatus(String(error)) }
    finally { setBusy(false) }
  }

  return <details className="rounded-lg border p-3">
    <summary className="cursor-pointer text-sm font-medium">开发者选项</summary>
    <div className="mt-3 space-y-3">
      <fieldset disabled={!desktop || disabled || busy} className="space-y-3 disabled:opacity-50">
        <legend className="text-sm font-medium">pill</legend>
        <div className="flex flex-wrap gap-x-3 gap-y-2">
          {states.map(item => <label key={item.value} className="flex cursor-pointer items-center gap-1.5 text-sm">
            <input type="radio" name="pill" value={item.value} checked={state === item.value} onChange={() => { void preview(item.value) }} className="size-4 accent-primary" />
            {item.label}
          </label>)}
        </div>
      </fieldset>
      <p className="text-xs text-muted-foreground" role="status">{!desktop ? '请在桌面应用中控制原生 Pill。' : disabled ? '录音和识别期间暂停外观预览。' : status}</p>
    </div>
  </details>
}
