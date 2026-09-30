export interface HistoryEntry {
  id: string
  started_at_ms: number
  local_date: string
  utc_offset_minutes: number
  raw_text: string
  polished_text: string | null
  character_count: number
  audio_duration_ms: number
  deleting: boolean
}
export interface HistoryCursor { local_date: string; started_at_ms: number; id: string }
export interface HistoryPage { entries: HistoryEntry[]; next_cursor: HistoryCursor | null }
export interface DailyUsage { local_date: string; character_count: number; audio_duration_ms: number; recognition_count: number }
export interface Insights { character_count: number; audio_duration_ms: number; days: DailyUsage[] }
export interface TranscriptionOutcome { text: string; history_id: string | null; warnings: string[] }

export function localDate(date: Date): string {
  return `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, '0')}-${String(date.getDate()).padStart(2, '0')}`
}

export function dateLabel(date: string, today: string): string {
  const yesterday = new Date(`${today}T12:00:00`)
  yesterday.setDate(yesterday.getDate() - 1)
  return date === today ? '今天' : date === localDate(yesterday) ? '昨天' : date
}

export function recordingTime(entry: HistoryEntry): string {
  return new Date(entry.started_at_ms + entry.utc_offset_minutes * 60_000).toISOString().slice(11, 19)
}

export function durationLabel(milliseconds: number): string {
  const seconds = Math.floor(Math.abs(milliseconds) / 1000)
  const sign = milliseconds < 0 ? '−' : ''
  if (seconds < 60) return `${sign}${seconds} 秒`
  const minutes = Math.floor(seconds / 60)
  return minutes < 60 ? `${sign}${minutes} 分钟` : `${sign}${Math.floor(minutes / 60)} 小时 ${minutes % 60} 分钟`
}

export function usageLevel(count: number): number {
  return count === 0 ? 0 : count < 100 ? 1 : count < 500 ? 2 : count < 1000 ? 3 : 4
}

export function calendarWeeks(today: string): (string | null)[][] {
  const end = new Date(`${today}T12:00:00`)
  const start = new Date(end.getFullYear(), end.getMonth() - 5, 1, 12)
  const cursor = new Date(start)
  cursor.setDate(cursor.getDate() - (cursor.getDay() + 6) % 7)
  const weeks: (string | null)[][] = []
  while (cursor <= end) {
    const week: (string | null)[] = []
    for (let day = 0; day < 7; day++) {
      week.push(cursor < start || cursor > end ? null : localDate(cursor))
      cursor.setDate(cursor.getDate() + 1)
    }
    weeks.push(week)
  }
  return weeks
}

// Browser previews are disposable samples and never read desktop data.
const previewNow = new Date()
let previewEntries: HistoryEntry[] = Array.from({ length: 8 }, (_, index) => {
  const date = new Date(previewNow)
  date.setDate(date.getDate() - Math.floor(index / 3))
  date.setHours(16 - index % 3 * 3, 32, 8)
  return { id: `preview-${index}`, started_at_ms: date.getTime(), local_date: localDate(date), utc_offset_minutes: -date.getTimezoneOffset(),
    raw_text: '嗯明天上午十点开会请提前准备一下项目进展和需要讨论的问题', polished_text: '明天上午十点开会，请提前准备项目进展和需要讨论的问题。',
    character_count: 28, audio_duration_ms: 12_000, deleting: false }
})
const previewDays: DailyUsage[] = Array.from({ length: 180 }, (_, index) => {
  const date = new Date(previewNow)
  date.setDate(date.getDate() - index)
  const count = index % 5 === 0 ? 0 : (index * 137 + 84) % 1700
  return { local_date: localDate(date), character_count: count, audio_duration_ms: count * 500, recognition_count: Math.ceil(count / 120) }
})
export function previewHistory(): HistoryPage { return { entries: [...previewEntries], next_cursor: null } }
export function previewInsights(): Insights {
  return { character_count: previewDays.reduce((sum, day) => sum + day.character_count, 0), audio_duration_ms: previewDays.reduce((sum, day) => sum + day.audio_duration_ms, 0), days: previewDays }
}
export function deletePreviewEntry(id: string): void { previewEntries = previewEntries.filter(entry => entry.id !== id) }
export function previewText(id: string, kind: 'raw' | 'polished'): string {
  const entry = previewEntries.find(entry => entry.id === id)
  if (!entry) throw new Error('历史记录不存在')
  return kind === 'raw' ? entry.raw_text : entry.polished_text ?? ''
}
