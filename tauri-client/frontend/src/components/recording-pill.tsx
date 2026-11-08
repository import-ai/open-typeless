import { Check, LoaderCircle, X } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { previewPillError, type PillError, type PillState } from '@/lib/desktop'
import { waveformAmplitude } from '@/lib/waveform'

interface Props {
  state: PillState
  error?: PillError
  levels: readonly number[]
  onCancel?: () => void
  onDone?: () => void
  onDrag?: () => void
}

// Both the desktop overlay and debug gallery render this exact component.
export function RecordingPill({ state, error = previewPillError, levels, onCancel, onDone, onDrag }: Props) {
  const processing = state === 'processing'
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
      <div className="pill-waveform pill-draggable" onMouseDown={event => { if (event.button === 0) onDrag?.() }} aria-label={state === 'ready' ? '录音波形' : '平线'} role="img">
        {state === 'ready' ? levels.map((level, index) => (
          <span key={index} className="pill-waveform-bar" style={{ height: 3 + 15 * waveformAmplitude(level) }} />
        )) : <span className="pill-waveform-flat" />}
      </div>
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
