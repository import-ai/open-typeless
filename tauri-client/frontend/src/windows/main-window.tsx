import { useEffect, useRef, useState } from 'react'
import { Mic, Square } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { useTauriEvent } from '@/hooks/use-tauri-event'
import { commands, desktop, micLabels, type MicState } from '@/lib/desktop'

type Phase = 'idle' | 'starting' | 'recording' | 'processing'

export function MainWindow() {
  const [phase, setPhase] = useState<Phase>('idle')
  const phaseRef = useRef<Phase>('idle')
  const busy = useRef(false)
  const [mic, setMic] = useState<MicState>('disconnected')
  const [status, setStatus] = useState(desktop ? '就绪' : '界面预览 · 请在桌面应用中录音')
  const [shortcut, setShortcut] = useState('')
  const [savedShortcut, setSavedShortcut] = useState('')
  const [saving, setSaving] = useState(false)
  const [loaded, setLoaded] = useState(false)
  const changePhase = (next: Phase) => { phaseRef.current = next; setPhase(next) }
  const reset = () => { changePhase('idle'); setMic('disconnected') }
  const dismiss = async () => {
    try { await commands.dismissPill() } catch (error) { console.error(error) }
  }
  useEffect(() => {
    if (!desktop) return
    let active = true
    void commands.settings().then(settings => {
      if (!active) return
      setShortcut(settings.shortcut)
      setSavedShortcut(settings.shortcut)
      setLoaded(true)
    }).catch(error => { if (active) setStatus(String(error)) })
    return () => { active = false }
  }, [])

  async function start() {
    if (busy.current || phaseRef.current !== 'idle') return
    busy.current = true
    changePhase('starting')
    setMic('unready')
    setStatus('等待麦克风输出…')
    try {
      await commands.start()
      changePhase('recording')
      setStatus('录音中…')
    } catch (error) {
      setStatus(String(error))
      reset()
      await dismiss()
    } finally { busy.current = false }
  }
  async function transcribe(path: string) {
    changePhase('processing')
    setStatus('正在上传和识别…')
    try {
      const text = await commands.transcribe(path)
      setStatus(text || '未识别到文字')
    } catch (error) {
      setStatus(String(error))
    } finally { reset(); await dismiss() }
  }
  async function stop() {
    if (busy.current || phaseRef.current !== 'recording') return
    busy.current = true
    changePhase('processing')
    try { await transcribe(await commands.stop()) }
    catch (error) { setStatus(String(error)); reset(); await dismiss() }
    finally { busy.current = false }
  }
  async function cancel() {
    if (busy.current || phaseRef.current !== 'recording') return
    busy.current = true
    try { await commands.cancel(); reset(); setStatus('已取消录音') }
    catch (error) { setStatus(String(error)) }
    finally { busy.current = false }
  }

  useTauriEvent<MicState>('mic-state', setMic)
  useTauriEvent('recording-starting', () => {
    changePhase('starting'); setMic('unready'); setStatus('等待麦克风输出…')
  })
  useTauriEvent('recording-started', () => { changePhase('recording'); setStatus('录音中…') })
  useTauriEvent<string>('recording-stopped', (path) => {
    if (busy.current) return
    busy.current = true
    void transcribe(path).finally(() => { busy.current = false })
  })
  useTauriEvent('stop-requested', () => { void stop() })
  useTauriEvent('cancel-requested', () => { void cancel() })
  useTauriEvent('recording-cancelled', () => { reset(); setStatus('已取消录音') })
  useTauriEvent<string>('recording-error', (error) => { reset(); setStatus(error); void dismiss() })

  async function saveShortcut() {
    setSaving(true)
    try {
      const value = shortcut.trim()
      await commands.shortcut(value)
      setSavedShortcut(value)
      setStatus('快捷键已更新')
    } catch (error) { setStatus(String(error)) }
    finally { setSaving(false) }
  }

  return <main className="mx-auto flex max-w-xl flex-col gap-5 p-6">
    <header>
      <h1 className="text-xl font-semibold tracking-tight">OpenTypeless</h1>
      <p className="mt-1 text-sm text-muted-foreground">按快捷键开始录音，再按一次完成输入。</p>
    </header>
    <Card>
      <CardHeader>
        <CardTitle className="text-sm">语音输入</CardTitle>
        <CardDescription>{savedShortcut || '快捷键加载后显示'}</CardDescription>
      </CardHeader>
      <CardContent className="space-y-4">
        <div className="flex items-center justify-between gap-3">
          <Button disabled={!loaded || phase === 'starting' || phase === 'processing'} onClick={() => { void (phase === 'recording' ? stop() : start()) }}>
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
        <Input id="shortcut" value={shortcut} onChange={event => setShortcut(event.target.value)} placeholder="Command+Shift+Space" disabled={!loaded} />
        <Button variant="outline" disabled={!loaded || saving || !shortcut.trim()} onClick={() => { void saveShortcut() }}>{saving ? '保存中…' : '保存'}</Button>
      </div>
    </section>
  </main>
}
