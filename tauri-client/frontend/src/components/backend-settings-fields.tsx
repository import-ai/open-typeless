import { useState } from 'react'
import { Input } from '@/components/ui/input'
import { useInputShortcutGuard } from '@/hooks/use-input-shortcut-guard'

interface Props {
  serverUrl: string
  apiKey: string
  disabled: boolean
  status: string
  onServerUrlChange: (value: string) => void
  onApiKeyChange: (value: string) => void
  onSave: () => void
  onInputActiveChange: (active: boolean) => void
}

export function BackendSettingsFields(props: Props) {
  const [focused, setFocused] = useState(false)
  const guard = useInputShortcutGuard(focused, props.onInputActiveChange)
  const onEnter = (event: React.KeyboardEvent<HTMLInputElement>) => {
    if (event.key === 'Enter' && !event.nativeEvent.isComposing && event.keyCode !== 229) event.currentTarget.blur()
  }
  return <div className="space-y-5"
    onFocusCapture={() => setFocused(true)}
    onBlurCapture={event => { if (!event.currentTarget.contains(event.relatedTarget)) setFocused(false) }}>
    <div className="space-y-2">
      <label htmlFor="server-url" className="text-sm font-medium">后端地址</label>
      <Input id="server-url" type="url" value={props.serverUrl}
        placeholder="https://example.com/api/v1"
        disabled={props.disabled} readOnly={!guard.ready}
        onChange={event => props.onServerUrlChange(event.target.value)}
        onBlur={props.onSave} onKeyDown={onEnter} />
    </div>
    <div className="space-y-2">
      <label htmlFor="backend-api-key" className="text-sm font-medium">API key</label>
      <Input id="backend-api-key" type="password" value={props.apiKey}
        placeholder="输入服务提供者给你的 API key"
        autoComplete="off" autoCapitalize="none" spellCheck={false}
        aria-describedby="backend-settings-help"
        disabled={props.disabled} readOnly={!guard.ready}
        onChange={event => props.onApiKeyChange(event.target.value)}
        onBlur={props.onSave} onKeyDown={onEnter} />
      <p id="backend-settings-help" role="status" className="text-xs text-muted-foreground">
        {guard.error || props.status || '按回车或移开焦点自动保存。'}
      </p>
    </div>
  </div>
}
