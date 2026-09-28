import { useEffect, useRef, useState } from 'react'
import { ShortcutKeys } from '@/components/shortcut-keys'
import { Button } from '@/components/ui/button'
import { Kbd } from '@/components/ui/kbd'
import { commands, desktop } from '@/lib/desktop'
import { ShortcutCapture } from '@/lib/shortcut-capture'

export function ShortcutRecorder({ value, disabled, onChange, onCapturingChange }: {
  value: string
  disabled: boolean
  onChange: (value: string) => void
  onCapturingChange: (value: boolean) => void
}) {
  const [listening, setListening] = useState(false)
  const [pending, setPending] = useState(false)
  const [preview, setPreview] = useState('')
  const [error, setError] = useState('')
  const active = useRef(false)
  const button = useRef<HTMLButtonElement>(null)
  const capture = useRef(new ShortcutCapture(navigator.userAgent.includes('Mac')))

  async function finish(shortcut?: string) {
    if (!active.current) return
    active.current = false
    setListening(false)
    setPending(true)
    try {
      if (desktop) await commands.captureShortcut(false)
      if (shortcut) onChange(shortcut)
    } catch (error) { setError(String(error)) }
    finally { setPending(false); onCapturingChange(false) }
  }

  async function begin() {
    if (active.current || pending) return
    active.current = true
    onCapturingChange(true)
    setPending(true)
    setError('')
    setPreview('')
    capture.current = new ShortcutCapture(navigator.userAgent.includes('Mac'))
    try {
      if (desktop) await commands.captureShortcut(true)
      if (!active.current || document.activeElement !== button.current) {
        // Focus may have moved while native shortcuts were being suspended.
        if (desktop) await commands.captureShortcut(false)
        active.current = false
        onCapturingChange(false)
        return
      }
      setListening(true)
    } catch (error) { active.current = false; onCapturingChange(false); setError(String(error)) }
    finally { setPending(false) }
  }

  useEffect(() => () => {
    if (active.current && desktop) void commands.captureShortcut(false).catch(console.error)
    active.current = false
  }, [])

  useEffect(() => {
    if (!listening) return
    const outside = (event: PointerEvent) => {
      if (event.target instanceof Node && !button.current?.contains(event.target)) void finish()
    }
    const blur = () => { void finish() }
    document.addEventListener('pointerdown', outside, true)
    window.addEventListener('blur', blur)
    return () => {
      document.removeEventListener('pointerdown', outside, true)
      window.removeEventListener('blur', blur)
    }
  }, [listening])

  return <div className="min-w-0 flex-1 space-y-2">
    <Button
      ref={button}
      id="shortcut"
      variant="outline"
      className={`h-auto min-h-8 w-full justify-start py-1.5 ${listening ? 'border-ring ring-2 ring-ring/30' : ''}`}
      disabled={disabled}
      aria-label={listening ? '请按下快捷键' : '录入语音输入快捷键'}
      aria-describedby="shortcut-help"
      aria-busy={pending}
      onClick={event => { event.currentTarget.focus(); void begin() }}
      onBlur={() => { void finish() }}
      onKeyDown={event => {
        if (!active.current) return
        event.preventDefault()
        event.stopPropagation()
        if (event.key === 'Escape') { void finish(); return }
        if (!listening) return
        const result = capture.current.keyDown(event.nativeEvent)
        setPreview(result.preview)
        setError(result.error ?? '')
      }}
      onKeyUp={event => {
        if (!active.current) return
        event.preventDefault()
        event.stopPropagation()
        if (!listening) return
        const result = capture.current.keyUp(event.nativeEvent)
        if (!result.done) return
        if (result.shortcut) { void finish(result.shortcut) }
        else {
          setError(result.error ?? '')
          setPreview('')
          capture.current = new ShortcutCapture(navigator.userAgent.includes('Mac'))
        }
      }}
    >
      {listening ? (preview ? <ShortcutKeys shortcut={preview} /> : '请按下快捷键…') : <ShortcutKeys shortcut={value} />}
    </Button>
    <p id="shortcut-help" className="text-xs text-muted-foreground" aria-live="polite">
      {error || (listening ? <>松开按键完成录入，<Kbd>Esc</Kbd> 取消。</> : '点击后直接按下快捷键，再点击保存。')}
    </p>
  </div>
}
