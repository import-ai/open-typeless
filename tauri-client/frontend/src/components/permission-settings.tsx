import { useEffect, useState } from 'react'
import { Check, RefreshCw } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { commands, desktop, type PermissionId, type PermissionState, type PermissionStatus } from '@/lib/desktop'

const labels: Record<PermissionStatus, string> = {
  granted: '已授权', not_determined: '尚未请求', restricted: '系统限制', denied: '未授权',
  not_granted: '未授权', unsupported: '当前平台不适用', unknown: '需在系统设置中确认',
}
const permissions: { id: PermissionId; title: string; description: string; remote?: boolean }[] = [
  { id: 'microphone', title: '麦克风', description: '使用电脑麦克风录音。使用遥控器麦克风时无需此权限。' },
  { id: 'accessibility', title: '辅助功能', description: '将识别结果粘贴到当前应用，并支持遥控器返回键退格。' },
  { id: 'bluetooth', title: '蓝牙', description: '连接小米遥控器并接收语音。', remote: true },
  { id: 'input_monitoring', title: '输入监控', description: '监听小米遥控器返回键，用于取消识别或在输入框中退格。', remote: true },
]

export function PermissionSettings({ disabled }: { disabled: boolean }) {
  const [states, setStates] = useState<PermissionState[]>([])
  const [pending, setPending] = useState<PermissionId | null>(null)
  const [error, setError] = useState('')
  const [refresh, setRefresh] = useState(0)
  useEffect(() => {
    let active = true
    async function update() {
      try {
        const next = await commands.permissions()
        if (active) setStates(next)
      } catch (error) { if (active) setError(String(error)) }
    }
    void update()
    const timer = window.setInterval(() => { void update() }, 2000)
    window.addEventListener('focus', update)
    return () => { active = false; window.clearInterval(timer); window.removeEventListener('focus', update) }
  }, [refresh])

  async function authorize(id: PermissionId) {
    setPending(id)
    setError('')
    try { await commands.requestPermission(id); setRefresh(value => value + 1) }
    catch (error) { setError(String(error)) }
    finally { setPending(null) }
  }

  return <div className="space-y-5">
    <Card>
      <CardHeader>
        <CardTitle>系统权限</CardTitle>
        <CardDescription>查看授权状态，并为需要的功能授权。基础权限会在 macOS 启动时请求；遥控器权限在启用该模式时请求。</CardDescription>
      </CardHeader>
      <CardContent className="space-y-4">
        <div className="flex items-center justify-between gap-3">
          <p className="text-xs text-muted-foreground">状态会自动刷新</p>
          <Button variant="ghost" size="sm" onClick={() => { setError(''); setRefresh(value => value + 1) }}><RefreshCw />刷新状态</Button>
        </div>
        <ul className="divide-y rounded-lg border px-4">
          {permissions.map(permission => {
            const state = states.find(state => state.id === permission.id)?.status
            return <li key={permission.id} className="space-y-2 py-4">
              <div className="flex flex-wrap items-center justify-between gap-3">
                <h3 className="font-medium">{permission.title}{permission.remote && <span className="ml-2 text-xs font-normal text-muted-foreground">仅遥控器模式</span>}</h3>
                <span role="status" className={`inline-flex items-center gap-1 text-xs ${state === 'granted' ? 'text-green-700 dark:text-green-400' : 'text-muted-foreground'}`}>
                  {state === 'granted' && <Check className="size-3.5" />}{state ? labels[state] : '正在检查…'}
                </span>
              </div>
              <p className="text-xs leading-relaxed text-muted-foreground">{permission.description}</p>
              <Button variant="outline" size="sm" disabled={disabled || !desktop || !state || pending !== null || state === 'granted' || state === 'unsupported' || state === 'restricted'} onClick={() => { void authorize(permission.id) }}>
                {pending === permission.id ? '正在请求…' : state === 'granted' ? '已授权' : state === 'restricted' ? '由系统策略管理' : state === 'unsupported' ? '无需授权' : state === 'unknown' ? '打开系统设置' : '去授权'}
              </Button>
            </li>
          })}
        </ul>
        <p className="text-xs leading-relaxed text-muted-foreground">授权按钮会请求系统授权，或打开对应的系统设置。曾拒绝的权限可能需要手动开启；辅助功能和输入监控授权后，请完全退出并重新打开应用。状态会自动刷新。</p>
        {!desktop && <p className="text-xs text-muted-foreground">当前为界面预览，请在桌面应用中查看真实权限并授权。</p>}
        {error && <p role="alert" className="text-sm text-destructive">{error}</p>}
      </CardContent>
    </Card>
  </div>
}
