import { invoke, isTauri } from '@tauri-apps/api/core'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { loadPreviewDictionary, savePreviewWord, deletePreviewWords, type DictionaryEntry } from './dictionary'

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

export interface RecordingFile { path: string; run_id: number }
export interface Settings { shortcut: string; server_url: string; api_key: string; shortcut_warning: string | null; settings_warning: string | null; developer_options: boolean }
export const commands = {
  dictionary: async () => desktop ? invoke<DictionaryEntry[]>('get_dictionary') : loadPreviewDictionary(),
  saveWord: async (text: string, id?: string) => desktop ? invoke<DictionaryEntry[]>('save_dictionary_entry', { text, id: id ?? null }) : savePreviewWord(id, text),
  deleteWords: async (ids: string[]) => desktop ? invoke<DictionaryEntry[]>('delete_dictionary_entries', { ids }) : deletePreviewWords(ids),
  checkServer: async (base: string, apiKey: string) => {
    if (desktop) return invoke<void>('check_server_connection')
    const response = await fetch(`${base}/health`, { signal: AbortSignal.timeout(3000), redirect: 'error', headers: apiKey ? { Authorization: `Bearer ${apiKey}` } : {} })
      .catch(() => { throw new Error('无法连接后端，请检查地址和服务状态') })
    if (response.status === 401 || response.status === 403) throw new Error('API key 无效或未设置，请检查后端设置')
    if (!response.ok || (await response.json()).status !== 'ok') throw new Error('后端健康检查未通过')
  },
  backendSettings: (url: string, apiKey: string) => invoke<void>('set_backend_settings', { url, apiKey }),
  settings: () => invoke<Settings>('get_settings'),
  start: () => invoke<void>('start_recording'),
  stop: () => invoke<RecordingFile>('stop_recording'),
  cancel: () => invoke<void>('cancel_recording'),
  transcribe: (file: RecordingFile) => invoke<string>('transcribe_file', { file }),
  captureShortcut: (capturing: boolean) => invoke<void>('set_shortcut_capture', { capturing }),
  shortcut: (shortcut: string) => invoke<void>('set_shortcut', { shortcut }),
  dismissPill: () => invoke<void>('dismiss_pill'),
  previewPill: (mode: PillState | 'hidden') => invoke<string>('debug_pill_preview', { mode }),
}
