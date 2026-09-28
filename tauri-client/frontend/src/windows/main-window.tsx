import { useCallback, useEffect, useRef, useState } from 'react'
import { Tabs } from 'radix-ui'
import { DictionaryPanel } from '@/components/dictionary-panel'
import { Mic, Square } from 'lucide-react'
import { DeveloperOptions } from '@/components/developer-options'
import { ShortcutKeys } from '@/components/shortcut-keys'
import { Kbd } from '@/components/ui/kbd'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { ShortcutRecorder } from '@/components/shortcut-recorder'
import { Input } from '@/components/ui/input'
import { useTauriEvent } from '@/hooks/use-tauri-event'
import { commands, desktop, micLabels, type MicState, type RecordingFile } from '@/lib/desktop'

type Phase = 'idle' | 'starting' | 'recording' | 'processing'
const previewShortcut = navigator.userAgent.includes('Mac') ? 'RCommand' : 'RControl'

export function MainWindow() {
  const [phase, setPhase] = useState<Phase>('idle')
  const phaseRef = useRef<Phase>('idle')
  const busy = useRef(false)
  const epoch = useRef(0)
  const pillVisible = useRef(false)
  const [mic, setMic] = useState<MicState>('disconnected')
  const [status, setStatus] = useState(desktop ? '就绪' : '界面预览 · 请在桌面应用中录音')
  const [shortcut, setShortcut] = useState(desktop ? '' : previewShortcut)
  const [serverUrl, setServerUrl] = useState('')
  const savedServerUrl = useRef('')
  const [configuredServerUrl, setConfiguredServerUrl] = useState('')
  const [connection, setConnection] = useState<'checking' | 'ready' | 'error'>('checking')
  const [connectionError, setConnectionError] = useState('')
  const [serverStatus, setServerStatus] = useState('')
  const [settingsWarning, setSettingsWarning] = useState('')
  const [savingServer, setSavingServer] = useState(false)
  const [developerOptions, setDeveloperOptions] = useState(false)
  const [capturingShortcut, setCapturingShortcut] = useState(false)
  const capturingShortcutRef = useRef(false)
  const dictionaryInputActive = useRef(false)
  const onDictionaryInputActiveChange = useCallback((active: boolean) => { dictionaryInputActive.current = active }, [])
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
      setServerUrl(settings.server_url)
      savedServerUrl.current = settings.server_url
      setConfiguredServerUrl(settings.server_url)
      setDeveloperOptions(settings.developer_options)
      setLoaded(true)
      setSettingsWarning(settings.settings_warning ?? settings.shortcut_warning ?? '')
    }).catch(error => { if (active) setStatus(String(error)) })
    return () => { active = false }
  }, [])

  useEffect(() => {
    if (!configuredServerUrl || phase !== 'idle') return
    let active = true
    let timer: ReturnType<typeof setTimeout>
    setConnection('checking')
    const check = async () => {
      try {
        await commands.checkServer(configuredServerUrl)
        if (active) setConnection('ready')
      } catch (error) {
        if (active) { setConnection('error'); setConnectionError(error instanceof Error ? error.message : String(error)) }
      }
      if (active) timer = setTimeout(() => { void check() }, 30_000)
    }
    void check()
    return () => { active = false; clearTimeout(timer) }
  }, [configuredServerUrl, phase])

  const idleStatus = !configuredServerUrl ? '后端地址未设置'
    : connection === 'checking' ? '正在检查后端连接…'
    : connection === 'error' ? connectionError : status

  async function start() {
    if (busy.current || phaseRef.current !== 'idle') return
    if (!savedServerUrl.current) { setStatus('后端地址未设置'); return }
    const current = ++epoch.current
    busy.current = true
    changePhase('starting')
    setMic('unready')
    setStatus('等待麦克风输出…')
    try {
      await commands.checkServer(savedServerUrl.current)
      if (epoch.current !== current) return
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
    if (!capturingShortcutRef.current && !dictionaryInputActive.current) void (phaseRef.current === 'recording' ? stop() : start())
  })
  useTauriEvent('stop-requested', () => { void stop() })
  useTauriEvent('cancel-requested', () => { void cancel() })
  useTauriEvent('recording-cancelled', () => {
    // Invalidate every pending start/stop/upload continuation immediately.
    epoch.current++; busy.current = false; reset(); setStatus('已取消本次识别')
  })

  async function saveShortcut(value: string) {
    setSaving(true)
    try {
      if (desktop) await commands.shortcut(value)
      setShortcut(value)
      const settings = desktop ? await commands.settings() : null
      setSettingsWarning(settings?.settings_warning ?? settings?.shortcut_warning ?? '')
      setStatus('快捷键已更新')
    } catch (error) { setStatus(String(error)); throw error }
    finally { setSaving(false) }
  }

  async function saveServerUrl() {
    const value = serverUrl.trim().replace(/\/+$/, '')
    if (value === savedServerUrl.current) return
    setSavingServer(true)
    try {
      if (value) {
        const parsed = new URL(value)
        if (!['http:', 'https:'].includes(parsed.protocol) || !parsed.hostname || parsed.search || parsed.hash || parsed.username || parsed.password) throw new Error('请输入有效的 HTTP 或 HTTPS 后端地址')
      }
      if (desktop) await commands.serverUrl(value)
      savedServerUrl.current = value
      setConnection('checking')
      setConfiguredServerUrl(value)
      setStatus('就绪')
      setServerUrl(value)
      setServerStatus(value ? '后端地址已保存' : '后端地址未设置')
    } catch (error) { setServerStatus(String(error)) }
    finally { setSavingServer(false) }
  }

  return <main className="mx-auto flex max-w-xl flex-col gap-5 p-6">
    <header>
      <h1 className="text-xl font-semibold tracking-tight">Open Typeless</h1>
      <p className="mt-1 text-sm text-muted-foreground">按快捷键开始录音，再按一次完成输入。</p>
    </header>
    <Tabs.Root defaultValue="voice" className="space-y-5">
    <Tabs.List aria-label="主导航" className="grid grid-cols-3 rounded-lg bg-muted p-1">
      <Tabs.Trigger value="voice" className="rounded-md px-3 py-1.5 text-sm text-muted-foreground outline-none focus-visible:ring-2 focus-visible:ring-ring data-[state=active]:bg-background data-[state=active]:text-foreground data-[state=active]:shadow-sm">语音输入</Tabs.Trigger>
      <Tabs.Trigger value="dictionary" className="rounded-md px-3 py-1.5 text-sm text-muted-foreground outline-none focus-visible:ring-2 focus-visible:ring-ring data-[state=active]:bg-background data-[state=active]:text-foreground data-[state=active]:shadow-sm">词典</Tabs.Trigger>
      <Tabs.Trigger value="settings" className="rounded-md px-3 py-1.5 text-sm text-muted-foreground outline-none focus-visible:ring-2 focus-visible:ring-ring data-[state=active]:bg-background data-[state=active]:text-foreground data-[state=active]:shadow-sm">设置</Tabs.Trigger>
    </Tabs.List>
    <Tabs.Content value="voice" className="space-y-5 outline-none">
    <Card>
      <CardHeader>
        <CardTitle className="text-sm">语音输入</CardTitle>
        <CardDescription className="flex flex-wrap items-center gap-x-2 gap-y-1">
          {shortcut ? <><ShortcutKeys shortcut={shortcut} /><span>开始 / 完成</span></> : '快捷键加载后显示'}
          <span className="inline-flex items-center gap-1"><Kbd>Esc</Kbd>取消</span>
        </CardDescription>
      </CardHeader>
      <CardContent className="space-y-4">
        <div className="flex items-center justify-between gap-3">
          <Button disabled={!loaded || savingServer || capturingShortcut || phase === 'starting' || phase === 'processing'} onClick={() => { void (phase === 'recording' ? stop() : start()) }}>
            {phase === 'recording' ? <Square /> : <Mic />}
            {phase === 'recording' ? '停止录音' : phase === 'starting' ? '连接中…' : phase === 'processing' ? '识别中…' : '开始录音'}
          </Button>
          <span className="flex items-center gap-2 text-xs text-muted-foreground">
            <span className={`size-2 rounded-full ${mic === 'ready' ? 'bg-red-500' : 'bg-zinc-400'}`} />
            麦克风{micLabels[mic]}
          </span>
        </div>
        <p role="status" className="min-h-5 break-words text-sm text-muted-foreground">{phase === 'idle' ? idleStatus : status}</p>
      </CardContent>
    </Card>
    </Tabs.Content>
    <Tabs.Content value="dictionary" className="outline-none"><DictionaryPanel onInputActiveChange={onDictionaryInputActiveChange} /></Tabs.Content>
    <Tabs.Content value="settings" className="space-y-5 outline-none">
    <section className="space-y-5" aria-labelledby="settings-title">
      <h2 id="settings-title" className="text-base font-semibold">设置</h2>
      <section className="space-y-3" aria-labelledby="shortcut-label">
        <h2 id="shortcut-label" className="text-sm font-medium">语音输入快捷键</h2>
        <div className="flex gap-2">
          <ShortcutRecorder
            value={shortcut}
            disabled={(desktop && !loaded) || saving || phase !== 'idle'}
            onChange={saveShortcut}
            onCapturingChange={value => { capturingShortcutRef.current = value; setCapturingShortcut(value) }}
          />
        </div>
      </section>
      <div className="space-y-2">
        <h2 id="server-url-label" className="text-sm font-medium">后端地址</h2>
        <Input
          aria-labelledby="server-url-label"
          aria-describedby="server-url-help"
          type="url"
          value={serverUrl}
          placeholder="https://example.com/api/v1"
          disabled={(desktop && !loaded) || savingServer || phase !== 'idle'}
          onChange={event => { setServerUrl(event.target.value); setServerStatus('') }}
          onBlur={() => { void saveServerUrl() }}
          onKeyDown={event => { if (event.key === 'Enter') event.currentTarget.blur() }}
        />
        <p id="server-url-help" role="status" className="text-xs text-muted-foreground">{serverStatus || '按回车或移开焦点自动保存。'}</p>
      </div>
      {settingsWarning && <p role="status" className="mt-3 text-xs text-destructive">{settingsWarning}</p>}
    </section>
    {developerOptions && <DeveloperOptions disabled={phase !== 'idle' || capturingShortcut} />}
    </Tabs.Content>
    </Tabs.Root>
  </main>
}
