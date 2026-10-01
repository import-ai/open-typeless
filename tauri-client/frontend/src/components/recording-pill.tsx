import { Check, LoaderCircle, X } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { previewPillError, type PillError, type PillState } from '@/lib/desktop'

const heights = [9, 11, 15, 19, 23, 29, 34, 26, 19, 13, 10]
interface Props {
  state: PillState
  error?: PillError
  level?: number
  onCancel?: () => void
  onDone?: () => void
  onDrag?: () => void
}

// Both the desktop overlay and debug gallery render this exact component.
export function RecordingPill({ state, error = previewPillError, level = 0, onCancel, onDone, onDrag }: Props) {
  const processing = state === 'processing'
  const amplitude = Math.max(0, Math.min(1, level * 12))
  // Keep both shapes at fixed sizes. Resizing one composited surface from the
  // spinner back to the capsule leaves stale clipping in macOS WKWebView.
  return <div className="pill-shapes">
    <div className="recording-pill processing pill-draggable" onMouseDown={event => { if (event.button === 0) onDrag?.() }} role="status" aria-label="识别中" aria-hidden={!processing} style={{ opacity: processing ? 1 : 0, pointerEvents: processing ? 'auto' : 'none' }}>
      <LoaderCircle className="size-5 animate-spin" aria-hidden="true" />
    </div>
    <div className="recording-pill" data-state={state} aria-hidden={processing} inert={processing} style={{ opacity: processing ? 0 : 1 }}>
      <Button variant="ghost" size="icon" className="pill-cancel" aria-label={state === 'error' ? '关闭错误提示' : '取消录音'} onClick={onCancel}>
        <X aria-hidden="true" />
      </Button>
      {state === 'error' ? <span className="pill-error pill-draggable" role="alert" aria-label={error.message} title={error.message} onMouseDown={event => { if (event.button === 0) onDrag?.() }}>
        {error.label}
      </span> : <>
      <svg className="pill-waveform pill-draggable" onMouseDown={event => { if (event.button === 0) onDrag?.() }} viewBox="0 0 100 40" aria-label={state === 'ready' ? '录音波形' : '平线'} role="img">
        {state === 'ready' ? heights.map((height, index) => {
          const h = Math.max(2, height * amplitude)
          return <rect key={index} x={12 + index * 7} y={(40 - h) / 2} width="3" height={h} fill="currentColor" />
        }) : <path d="M12 20H85" stroke="currentColor" strokeWidth="1.5" />}
      </svg>
      {state === 'ready' ? (
        <Button variant="ghost" size="icon" className="pill-done" aria-label="完成录音" onClick={onDone}>
          <Check aria-hidden="true" />
        </Button>
      ) : (
        <span className="pill-waiting pill-draggable" onMouseDown={event => { if (event.button === 0) onDrag?.() }} role="status" aria-label={state === 'disconnected' ? '等待麦克风连接' : '等待麦克风就绪'}>
          <LoaderCircle className="size-5 animate-spin" aria-hidden="true" />
        </span>
      )}
      </>}
    </div>
  </div>
}
