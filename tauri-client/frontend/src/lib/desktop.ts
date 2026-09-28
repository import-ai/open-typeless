import { invoke, isTauri } from '@tauri-apps/api/core'
import { getCurrentWindow } from '@tauri-apps/api/window'

export type MicState = 'disconnected' | 'unready' | 'ready'
export type PillState = MicState | 'processing'
export const micLabels: Record<MicState, string> = {
  disconnected: '未连接', unready: '未就绪', ready: '已就绪',
}
export const desktop = isTauri()
// Browser-only URLs provide a preview. Native windows always use their own label.
export const windowLabel = desktop
  ? getCurrentWindow().label
  : new URLSearchParams(location.search).get('view') ?? 'main'

export interface Settings { shortcut: string; server_url: string }
export const commands = {
  settings: () => invoke<Settings>('get_settings'),
  start: () => invoke<void>('start_recording'),
  stop: () => invoke<string>('stop_recording'),
  cancel: () => invoke<void>('cancel_recording'),
  transcribe: (path: string) => invoke<string>('transcribe_file', { path }),
  shortcut: (shortcut: string) => invoke<void>('set_shortcut', { shortcut }),
  dismissPill: () => invoke<void>('dismiss_pill'),
  previewPill: (mode: PillState | 'hidden') => invoke<string>('debug_pill_preview', { mode }),
}
