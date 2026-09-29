export interface DictionaryEntry {
  id: string
  text: string
  created_at: number
  updated_at: number
}

export const MAX_HOTWORDS_BYTES = 1000
const key = (text: string) => text.replace(/[A-Z]/g, letter => letter.toLowerCase())
export const dictionaryMatches = (text: string, query: string) => key(text).includes(key(query.trim()))
export const serializeHotwords = (entries: DictionaryEntry[]) => entries.map(entry => entry.text).join('\n')

export function validateWord(entries: DictionaryEntry[], text: string, id?: string): string {
  const value = text.trim()
  if (!value) return '词汇不能为空'
  if (/[\u0000-\u001f\u007f-\u009f\u2028\u2029]/u.test(value)) return '请填写单个词汇，不要包含换行或控制字符'
  if (entries.some(entry => entry.id !== id && key(entry.text) === key(value))) return '该词汇已存在'
  const words = entries.filter(entry => entry.id !== id).map(entry => entry.text)
  words.push(value)
  if (new TextEncoder().encode(words.join('\n')).length > MAX_HOTWORDS_BYTES) return '词典容量已满，请缩短词汇或删除不再使用的词条'
  return ''
}

// Browser previews use separate storage; the desktop source of truth is the Rust file store.
const previewKey = 'open-typeless.dictionary.preview.v1'
export function loadPreviewDictionary(): DictionaryEntry[] {
  const entries: DictionaryEntry[] = JSON.parse(localStorage.getItem(previewKey) ?? '[]')
  if (!Array.isArray(entries)) throw new Error('读取词典失败')
  const seen: DictionaryEntry[] = []
  for (const entry of entries) {
    if (!entry || typeof entry.id !== 'string' || typeof entry.text !== 'string' || !entry.id || seen.some(item => item.id === entry.id) || validateWord(seen, entry.text)) throw new Error('读取词典失败')
    seen.push(entry)
  }
  return entries
}
export function savePreviewWord(id: string | undefined, text: string): DictionaryEntry[] {
  const entries = loadPreviewDictionary()
  const error = validateWord(entries, text, id)
  if (error) throw new Error(error)
  const value = text.trim()
  if (id) {
    const entry = entries.find(entry => entry.id === id)
    if (!entry) throw new Error('词条不存在，请重新加载词典')
    entry.text = value
    entry.updated_at = Date.now()
  } else entries.unshift({ id: crypto.randomUUID(), text: value, created_at: Date.now(), updated_at: Date.now() })
  localStorage.setItem(previewKey, JSON.stringify(entries))
  return entries
}
export function deletePreviewWords(ids: string[]): DictionaryEntry[] {
  const entries = loadPreviewDictionary().filter(entry => !ids.includes(entry.id))
  localStorage.setItem(previewKey, JSON.stringify(entries))
  return entries
}
