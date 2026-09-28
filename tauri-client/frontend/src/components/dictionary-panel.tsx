import { useEffect, useRef, useState } from 'react'
import { Checkbox, DropdownMenu } from 'radix-ui'
import { BookOpen, Check, Minus, MoreHorizontal, Pencil, Plus, Search, Trash2 } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Dialog } from '@/components/ui/dialog'
import { commands } from '@/lib/desktop'
import { dictionaryMatches, validateWord, type DictionaryEntry } from '@/lib/dictionary'
import { useInputShortcutGuard } from '@/hooks/use-input-shortcut-guard'

function SelectionBox({ checked, label, onChange }: { checked: boolean | 'indeterminate'; label: string; onChange: () => void }) {
  return <Checkbox.Root checked={checked} onCheckedChange={onChange} onClick={event => event.stopPropagation()} aria-label={label}
    className="flex size-4 shrink-0 items-center justify-center rounded border border-input bg-background outline-none focus-visible:ring-2 focus-visible:ring-ring data-[state=checked]:border-primary data-[state=checked]:bg-primary data-[state=checked]:text-primary-foreground data-[state=indeterminate]:bg-primary data-[state=indeterminate]:text-primary-foreground">
    <Checkbox.Indicator>{checked === 'indeterminate' ? <Minus className="size-3" /> : <Check className="size-3" />}</Checkbox.Indicator>
  </Checkbox.Root>
}

export function DictionaryPanel({ onInputActiveChange }: { onInputActiveChange: (active: boolean) => void }) {
  const [entries, setEntries] = useState<DictionaryEntry[]>([])
  const [loaded, setLoaded] = useState(false)
  const [loadError, setLoadError] = useState('')
  const [query, setQuery] = useState('')
  const [searchFocused, setSearchFocused] = useState(false)
  const [managing, setManaging] = useState(false)
  const [selected, setSelected] = useState<Set<string>>(new Set())
  const [editor, setEditor] = useState<{ id?: string; text: string } | null>(null)
  const [deleting, setDeleting] = useState<DictionaryEntry[] | null>(null)
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState('')
  const [notice, setNotice] = useState('')
  const addButton = useRef<HTMLButtonElement>(null)
  const menuButtons = useRef(new Map<string, HTMLButtonElement>())
  const opener = useRef<HTMLElement | null>(null)
  const openingDialog = useRef(false)
  const busy = useRef(false)
  const composing = useRef(false)
  const guard = useInputShortcutGuard(editor !== null || deleting !== null || searchFocused, onInputActiveChange)
  const visible = entries.filter(entry => dictionaryMatches(entry.text, query))
  const validation = editor ? validateWord(entries, editor.text, editor.id) : ''

  async function reload() {
    setLoadError('')
    try { setEntries(await commands.dictionary()); setLoaded(true) }
    catch (error) { setLoadError(String(error)); setLoaded(false) }
  }
  useEffect(() => { void reload() }, [])

  function edit(entry?: DictionaryEntry, trigger?: HTMLElement) {
    opener.current = trigger ?? addButton.current
    setError(''); setNotice(''); composing.current = false
    setEditor(entry ? { id: entry.id, text: entry.text } : { text: '' })
  }
  function confirmDelete(words: DictionaryEntry[], trigger?: HTMLElement) {
    opener.current = trigger ?? addButton.current
    setError(''); setNotice(''); setDeleting(words)
  }
  function restoreFocus(event: Event) {
    event.preventDefault()
    const target = opener.current
    if (target?.isConnected && !target.matches(':disabled')) target.focus()
    else addButton.current?.focus()
  }
  function toggle(id: string) {
    setSelected(current => {
      const next = new Set(current)
      if (next.has(id)) next.delete(id); else next.add(id)
      return next
    })
  }
  async function save() {
    if (!editor || validation || busy.current) return
    busy.current = true; setSaving(true); setError('')
    try {
      setEntries(await commands.saveWord(editor.text, editor.id))
      setNotice(editor.id ? '词汇已更新，下次录音生效。' : '词汇已添加，下次录音生效。')
      setEditor(null)
    } catch (error) { setError(String(error)) }
    finally { busy.current = false; setSaving(false) }
  }
  async function remove() {
    if (!deleting || busy.current) return
    busy.current = true; setSaving(true); setError('')
    try {
      setEntries(await commands.deleteWords(deleting.map(entry => entry.id)))
      setNotice(`已删除 ${deleting.length} 个词汇，下次录音生效。`)
      setSelected(new Set()); setDeleting(null)
    } catch (error) { setError(String(error)) }
    finally { busy.current = false; setSaving(false) }
  }

  return <section className="space-y-4" aria-labelledby="dictionary-title">
    <div>
      <div className="flex items-center justify-between gap-3">
        <h2 id="dictionary-title" className="text-base font-semibold">词典</h2>
        <Button ref={addButton} disabled={!loaded} onClick={() => edit()}><Plus />添加</Button>
      </div>
      <p className="mt-2 text-sm leading-relaxed text-muted-foreground">添加常用词，提高专有名词的识别准确度。</p>
    </div>
    <div className="relative">
      <Search className="pointer-events-none absolute left-3 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" />
      <Input aria-label="搜索词汇" placeholder="搜索词汇…" className="pl-9" value={query} disabled={!loaded}
        readOnly={searchFocused && !guard.ready}
        onFocus={() => setSearchFocused(true)} onBlur={() => setSearchFocused(false)}
        onChange={event => { setQuery(event.target.value); setSelected(new Set()); setNotice('') }} />
    </div>
    {loadError ? <div role="alert" className="space-y-2 text-sm text-destructive"><p>{loadError}</p><Button variant="outline" onClick={() => { void reload() }}>重新加载</Button></div>
      : !loaded ? <p role="status" className="text-sm text-muted-foreground">正在加载词典…</p> : <>
        <div className="flex min-h-8 items-center justify-between gap-2 text-xs text-muted-foreground">
          {managing ? <>
            <div className="flex items-center gap-2">
              <SelectionBox label="全选搜索结果" checked={selected.size === 0 ? false : selected.size === visible.length ? true : 'indeterminate'}
                onChange={() => setSelected(selected.size === visible.length ? new Set() : new Set(visible.map(entry => entry.id)))} />
              <span>已选择 {selected.size} 个词</span>
            </div>
            <div className="flex gap-1">
              <Button variant="destructive" size="sm" disabled={selected.size === 0} onClick={event => confirmDelete(entries.filter(entry => selected.has(entry.id)), event.currentTarget)}>删除</Button>
              <Button variant="ghost" size="sm" onClick={() => { setManaging(false); setSelected(new Set()) }}>完成</Button>
            </div>
          </> : <>
            <span>{query.trim() ? `找到 ${visible.length} 个词 · 共 ${entries.length} 个` : `共 ${entries.length} 个词`}</span>
            <Button variant="ghost" size="sm" disabled={entries.length === 0} onClick={() => setManaging(true)}>批量管理</Button>
          </>}
        </div>
        {visible.length > 0 ? <ul className="space-y-2">
          {visible.map(entry => <li key={entry.id} onClick={managing ? () => toggle(entry.id) : undefined} className={`flex min-h-12 items-center gap-2 rounded-xl border px-3 ${selected.has(entry.id) ? 'border-foreground bg-muted' : 'bg-background'}`}>
            {managing ? <>
              <button className="min-w-0 flex-1 self-stretch py-3 text-left text-sm outline-none focus-visible:underline" aria-label={`选择 ${entry.text}`}>
                <span className="block break-words [overflow-wrap:anywhere]">{entry.text}</span>
              </button>
              <SelectionBox label={entry.text} checked={selected.has(entry.id)} onChange={() => toggle(entry.id)} />
            </> : <>
              <button className="min-w-0 flex-1 self-stretch py-3 text-left text-sm outline-none hover:text-muted-foreground focus-visible:underline" onClick={event => edit(entry, event.currentTarget)} aria-label={`编辑 ${entry.text}`}>
                <span className="block break-words [overflow-wrap:anywhere]">{entry.text}</span>
              </button>
              <DropdownMenu.Root>
                <DropdownMenu.Trigger asChild><Button ref={element => { if (element) menuButtons.current.set(entry.id, element); else menuButtons.current.delete(entry.id) }} variant="ghost" size="icon-sm" aria-label={`${entry.text} 的更多操作`}><MoreHorizontal /></Button></DropdownMenu.Trigger>
                <DropdownMenu.Portal>
                  <DropdownMenu.Content align="end" sideOffset={5} className="z-30 min-w-32 rounded-xl border bg-popover p-1 text-sm text-popover-foreground shadow-lg"
                    onCloseAutoFocus={event => { if (openingDialog.current) { event.preventDefault(); openingDialog.current = false } }}>
                    <DropdownMenu.Item className="flex cursor-default items-center gap-2 rounded-lg px-3 py-2 outline-none focus:bg-accent" onSelect={() => { openingDialog.current = true; edit(entry, menuButtons.current.get(entry.id)) }}><Pencil className="size-4" />编辑</DropdownMenu.Item>
                    <DropdownMenu.Separator className="my-1 h-px bg-border" />
                    <DropdownMenu.Item className="flex cursor-default items-center gap-2 rounded-lg px-3 py-2 text-destructive outline-none focus:bg-destructive/10" onSelect={() => { openingDialog.current = true; confirmDelete([entry], menuButtons.current.get(entry.id)) }}><Trash2 className="size-4" />删除</DropdownMenu.Item>
                  </DropdownMenu.Content>
                </DropdownMenu.Portal>
              </DropdownMenu.Root>
            </>}
          </li>)}
        </ul> : <div className="flex flex-col items-center gap-3 rounded-xl border border-dashed px-5 py-10 text-center">
          <BookOpen className="size-6 text-muted-foreground" />
          <p className="text-sm">{entries.length ? '没有找到匹配的词汇' : '添加第一个词汇'}</p>
          <p className="text-xs leading-relaxed text-muted-foreground">{entries.length ? '试试其他关键词。' : '人名、产品名、专业术语，都可以加入词典。'}</p>
          {!entries.length && <Button variant="outline" onClick={() => edit()}><Plus />添加词汇</Button>}
        </div>}
      </>}
    <p role="status" className="min-h-4 text-xs text-muted-foreground">{notice}</p>
    {guard.error && <p role="alert" className="text-sm text-destructive">{guard.error}</p>}

    <Dialog open={editor !== null} onOpenChange={open => { if (!open) setEditor(null) }} title={editor?.id ? '编辑词汇' : '添加词汇'}
      description="填写你希望识别结果使用的准确写法。" busy={saving} onCloseAutoFocus={restoreFocus}>
      <form className="mt-5 space-y-4" onSubmit={event => { event.preventDefault(); if (!composing.current) void save() }}>
        <div className="space-y-2">
          <Input data-dialog-autofocus aria-label="词汇" aria-describedby="word-error" aria-invalid={Boolean(editor?.text && validation) || Boolean(error)}
            placeholder="例如：Open Typeless" value={editor?.text ?? ''} readOnly={!guard.ready} disabled={saving}
            onCompositionStart={() => { composing.current = true }} onCompositionEnd={() => { composing.current = false }}
            onKeyDown={event => { if (event.key === 'Enter' && (event.nativeEvent.isComposing || composing.current || event.nativeEvent.keyCode === 229)) event.preventDefault() }}
            onChange={event => { setEditor(current => current && { ...current, text: event.target.value }); setError('') }} />
          <p id="word-error" role="status" className="min-h-4 text-xs text-destructive">{error || guard.error || (editor?.text ? validation : '')}</p>
        </div>
        <div className="flex justify-end gap-2">
          <Button type="button" variant="outline" disabled={saving} onClick={() => setEditor(null)}>取消</Button>
          <Button type="submit" disabled={saving || !guard.ready || Boolean(validation)}>{saving ? '保存中…' : editor?.id ? '保存' : '添加'}</Button>
        </div>
      </form>
    </Dialog>
    <Dialog open={deleting !== null} onOpenChange={open => { if (!open) setDeleting(null) }} title={`删除 ${deleting?.length ?? 0} 个词汇？`}
      description="删除后，下次录音将不再使用这些词汇。此操作无法撤销。" busy={saving} onCloseAutoFocus={restoreFocus}>
      <ul className="mt-4 max-h-32 space-y-1 overflow-y-auto rounded-lg bg-muted p-3 text-sm">
        {deleting?.map(entry => <li key={entry.id} className="break-words [overflow-wrap:anywhere]">{entry.text}</li>)}
      </ul>
      {error && <p role="alert" className="mt-3 text-xs text-destructive">{error}</p>}
      <div className="mt-5 flex justify-end gap-2">
        <Button data-dialog-autofocus variant="outline" disabled={saving} onClick={() => setDeleting(null)}>取消</Button>
        <Button variant="destructive" disabled={saving} onClick={() => { void remove() }}>{saving ? '删除中…' : '删除'}</Button>
      </div>
    </Dialog>
  </section>
}
