import { useEffect, useRef, useState } from 'react'
import { Mic, Square } from 'lucide-react'
import { DeveloperOptions } from '@/components/developer-options'
import { ShortcutKeys } from '@/components/shortcut-keys'
import { Kbd } from '@/components/ui/kbd'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { ShortcutRecorder } from '@/components/shortcut-recorder'
import { Label } from '@/components/ui/label'
import { useTauriEvent } from '@/hooks/use-tauri-event'
import { commands, desktop, micLabels, type MicState, type RecordingFile } from '@/lib/desktop'

type Phase = 'idle' | 'starting' | 'recording' | 'processing'
const previewShortcut = navigator.userAgent.includes('Mac') ? 'RCommand' : 'Control+Shift+Space'

export function MainWindow() {
  const [phase, setPhase] = useState<Phase>('idle')
  const phaseRef = useRef<Phase>('idle')
  const busy = useRef(false)
  const epoch = useRef(0)
  const pillVisible = useRef(false)
  const [mic, setMic] = useState<MicState>('disconnected')
  const [status, setStatus] = useState(desktop ? '就绪' : '界面预览 · 请在桌面应用中录音')
  const [shortcut, setShortcut] = useState(desktop ? '' : previewShortcut)
  const [savedShortcut, setSavedShortcut] = useState(desktop ? '' : previewShortcut)
  const [capturingShortcut, setCapturingShortcut] = useState(false)
  const capturingShortcutRef = useRef(false)
  const [saving, setSaving] = useState(false)
  const [loaded, setLoaded] = useState(false)
  const changePhase = (next: Phase) => { phaseRef.current = next; setPhase(next) }
  const reset = () => { changePhase('idle'); setMic('disconnected') }
  useEffect(() => {
    if (!desktop) return
    let active = true
    void commands.settings().then(settings => {
      if (!active) return
      setShortcut(settings.shortcut)
      setSavedShortcut(settings.shortcut)
      setLoaded(true)
      if (settings.shortcut_warning) setStatus(settings.shortcut_warning)
    }).catch(error => { if (active) setStatus(String(error)) })
    return () => { active = false }
  }, [])

  async function start() {
    if (busy.current || phaseRef.current !== 'idle') return
    const current = ++epoch.current
    busy.current = true
    changePhase('starting')
    setMic('unready')
    setStatus('等待麦克风输出…')
    try {
      await commands.start()
      if (epoch.current !== current) return
      changePhase('recording')
      setStatus('录音中…')
    } catch (error) {
      if (epoch.current !== current) return
      setStatus(String(error)); reset()
    } finally { if (epoch.current === current) busy.current = false }
  }
  async function transcribe(file: RecordingFile, current: number) {
    if (epoch.current !== current) return
    changePhase('processing')
    setStatus('正在上传和识别…')
    try {
      const text = await commands.transcribe(file)
      if (epoch.current === current) setStatus(text || '未识别到文字')
    } catch (error) {
      if (epoch.current === current) setStatus(String(error))
    } finally { if (epoch.current === current) reset() }
  }
  async function stop() {
    if (busy.current || phaseRef.current !== 'recording') return
    const current = epoch.current
    busy.current = true
    changePhase('processing')
    try { await transcribe(await commands.stop(), current) }
    catch (error) { if (epoch.current === current) { setStatus(String(error)); reset() } }
    finally { if (epoch.current === current) busy.current = false }
  }
  async function cancel() {
    try { await commands.cancel() }
    catch (error) { setStatus(String(error)) }
  }

  useTauriEvent<MicState>('mic-state', setMic)
  useTauriEvent('pill-shown', () => { pillVisible.current = true })
  useTauriEvent('pill-hidden', () => { pillVisible.current = false })
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape' && pillVisible.current) {
        event.preventDefault()
        void commands.cancel().catch(console.error)
      }
    }
    document.addEventListener('keydown', onKeyDown)
    return () => document.removeEventListener('keydown', onKeyDown)
  }, [])
  useTauriEvent('toggle-requested', () => {
    if (!capturingShortcutRef.current) void (phaseRef.current === 'recording' ? stop() : start())
  })
  useTauriEvent('stop-requested', () => { void stop() })
  useTauriEvent('cancel-requested', () => { void cancel() })
  useTauriEvent('recording-cancelled', () => {
    // Invalidate every pending start/stop/upload continuation immediately.
    epoch.current++; busy.current = false; reset(); setStatus('已取消本次识别')
  })

  async function saveShortcut() {
    setSaving(true)
    try {
      const value = shortcut.trim()
      if (desktop) await commands.shortcut(value)
      setSavedShortcut(value)
      const settings = desktop ? await commands.settings() : null
      setStatus(settings?.shortcut_warning ?? '快捷键已更新')
    } catch (error) { setStatus(String(error)) }
    finally { setSaving(false) }
  }

  return <main className="mx-auto flex max-w-xl flex-col gap-5 p-6">
    <header>
      <h1 className="text-xl font-semibold tracking-tight">Open Typeless</h1>
      <p className="mt-1 text-sm text-muted-foreground">按快捷键开始录音，再按一次完成输入。</p>
    </header>
    <Card>
      <CardHeader>
        <CardTitle className="text-sm">语音输入</CardTitle>
        <CardDescription className="flex flex-wrap items-center gap-x-2 gap-y-1">
          {savedShortcut ? <><ShortcutKeys shortcut={savedShortcut} /><span>开始 / 完成</span></> : '快捷键加载后显示'}
          <span className="inline-flex items-center gap-1"><Kbd>Esc</Kbd>取消</span>
        </CardDescription>
      </CardHeader>
      <CardContent className="space-y-4">
        <div className="flex items-center justify-between gap-3">
          <Button disabled={!loaded || capturingShortcut || phase === 'starting' || phase === 'processing'} onClick={() => { void (phase === 'recording' ? stop() : start()) }}>
            {phase === 'recording' ? <Square /> : <Mic />}
            {phase === 'recording' ? '停止录音' : phase === 'starting' ? '连接中…' : phase === 'processing' ? '识别中…' : '开始录音'}
          </Button>
          <span className="flex items-center gap-2 text-xs text-muted-foreground">
            <span className={`size-2 rounded-full ${mic === 'ready' ? 'bg-red-500' : 'bg-zinc-400'}`} />
            麦克风{micLabels[mic]}
          </span>
        </div>
        <p role="status" className="min-h-5 break-words text-sm text-muted-foreground">{status}</p>
      </CardContent>
    </Card>
    <section className="space-y-3" aria-label="设置">
      <Label htmlFor="shortcut">语音输入快捷键</Label>
      <div className="flex gap-2">
        <ShortcutRecorder
          value={shortcut}
          disabled={(desktop && !loaded) || saving || phase !== 'idle'}
          onChange={setShortcut}
          onCapturingChange={value => { capturingShortcutRef.current = value; setCapturingShortcut(value) }}
        />
        <Button variant="outline" disabled={(desktop && !loaded) || capturingShortcut || saving || !shortcut.trim() || shortcut === savedShortcut} onClick={() => { void saveShortcut() }}>{saving ? '保存中…' : '保存'}</Button>
      </div>
    </section>
    <DeveloperOptions disabled={phase !== 'idle'} />
  </main>
}
