import { useCallback, useEffect, useRef, useState } from 'react'
import { DropdownMenu } from 'radix-ui'
import { Clock3, Copy, FolderOpen, MoreHorizontal, Trash2 } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Dialog } from '@/components/ui/dialog'
import { useHistoryRefresh } from '@/hooks/use-history-refresh'
import { useInputShortcutGuard } from '@/hooks/use-input-shortcut-guard'
import { commands } from '@/lib/desktop'
import { dateLabel, recordingTime, type HistoryCursor, type HistoryEntry, type HistoryPage } from '@/lib/history'

const menuItem = 'flex cursor-default items-center gap-2 rounded-lg px-3 py-2 outline-none focus:bg-accent data-[disabled]:pointer-events-none data-[disabled]:opacity-50'

export function HistoryPanel({ onInputActiveChange }: { onInputActiveChange: (active: boolean) => void }) {
  const [page, setPage] = useState<HistoryPage | null>(null)
  const [loading, setLoading] = useState(false)
  const [loadError, setLoadError] = useState('')
  const [error, setError] = useState('')
  const [notice, setNotice] = useState('')
  const [deleting, setDeleting] = useState<HistoryEntry | null>(null)
  const [saving, setSaving] = useState(false)
  const [menuOpen, setMenuOpen] = useState(false)
  const busy = useRef(false)
  const request = useRef(0)
  const title = useRef<HTMLHeadingElement>(null)
  const opener = useRef<HTMLButtonElement | null>(null)
  const openingDialog = useRef(false)
  const guard = useInputShortcutGuard(deleting !== null || menuOpen, onInputActiveChange)
  useEffect(() => () => { request.current++ }, [])
  const load = useCallback((cursor?: HistoryCursor) => {
    const current = ++request.current
    setLoading(true); setLoadError('')
    void commands.history(cursor).then(value => {
      if (request.current === current) setPage(previous => ({
        entries: cursor && previous ? [...previous.entries, ...value.entries] : value.entries,
        next_cursor: value.next_cursor,
      }))
    }).catch(error => { if (request.current === current) setLoadError(String(error)) })
      .finally(() => { if (request.current === current) setLoading(false) })
  }, [])
  const reload = useCallback(() => load(), [load])
  const today = useHistoryRefresh(reload)

  async function action(entry: HistoryEntry, kind: 'raw' | 'polished' | 'reveal') {
    if (busy.current) return
    busy.current = true; setError(''); setNotice('')
    try {
      if (kind === 'reveal') await commands.revealRecording(entry.id)
      else { await commands.copyHistoryText(entry.id, kind); setNotice('已复制') }
    } catch (error) { setError(String(error)) }
    finally { busy.current = false }
  }

  async function remove() {
    if (!deleting || busy.current) return
    busy.current = true; setSaving(true); setError('')
    try {
      await commands.deleteHistory(deleting.id)
      setDeleting(null); setNotice('已删除'); reload()
    } catch (error) { setError(String(error)); reload() }
    finally { busy.current = false; setSaving(false) }
  }

  return <section className="space-y-4" aria-labelledby="history-title">
    <h2 ref={title} id="history-title" tabIndex={-1} className="text-base font-semibold outline-none">历史记录</h2>
    {loadError && <div role="alert" className="space-y-2 text-sm text-destructive"><p>{loadError}</p><Button variant="outline" onClick={reload}>重新加载</Button></div>}
    {!page && loading && <p role="status" className="text-sm text-muted-foreground">正在加载历史记录…</p>}
    {page && !page.entries.length && !loadError && <div className="flex flex-col items-center gap-3 rounded-xl border border-dashed px-5 py-10 text-center">
      <Clock3 className="size-6 text-muted-foreground" /><p className="text-sm">暂无历史记录</p>
      <p className="text-xs text-muted-foreground">完成语音输入后，记录会保存在这里。</p>
    </div>}
    <div>
      {page?.entries.map((entry, index) => <div key={entry.id}>
        {(index === 0 || page.entries[index - 1].local_date !== entry.local_date) && <div className={`flex items-center gap-3 pb-2 ${index ? 'pt-6' : ''}`}>
          <h3 className="shrink-0 text-xs font-medium text-muted-foreground">{dateLabel(entry.local_date, today)}</h3><span className="h-px flex-1 bg-border" />
        </div>}
        <div className="flex items-start gap-2 border-b py-2">
          <time className="w-[58px] shrink-0 pt-1.5 text-xs tabular-nums text-muted-foreground" dateTime={new Date(entry.started_at_ms).toISOString()}>{recordingTime(entry)}</time>
          <details className="group min-w-0 flex-1 py-1 text-sm">
            <summary className="cursor-pointer list-none truncate rounded outline-none focus-visible:ring-2 focus-visible:ring-ring [&::-webkit-details-marker]:hidden">
              {entry.deleting ? '删除未完成，请重试' : entry.polished_text?.trim() ? entry.polished_text : entry.raw_text}
            </summary>
            <p className="mt-2 whitespace-pre-wrap break-words [overflow-wrap:anywhere]">{entry.polished_text?.trim() ? entry.polished_text : entry.raw_text}</p>
          </details>
          <DropdownMenu.Root onOpenChange={setMenuOpen}>
            <DropdownMenu.Trigger asChild><Button variant="ghost" size="icon-sm" aria-label={`${recordingTime(entry)} 的更多操作`}
              onPointerDown={event => { opener.current = event.currentTarget }} onKeyDown={event => { opener.current = event.currentTarget }}><MoreHorizontal /></Button></DropdownMenu.Trigger>
            <DropdownMenu.Portal>
              <DropdownMenu.Content align="end" sideOffset={5} className="z-30 min-w-40 rounded-xl border bg-popover p-1 text-sm text-popover-foreground shadow-lg"
                onCloseAutoFocus={event => { if (openingDialog.current) { event.preventDefault(); openingDialog.current = false } }}>
                <DropdownMenu.Item className={menuItem} disabled={entry.deleting} onSelect={() => { void action(entry, 'reveal') }}><FolderOpen className="size-4" />查看录音</DropdownMenu.Item>
                <DropdownMenu.Item className={menuItem} disabled={entry.deleting} onSelect={() => { void action(entry, 'raw') }}><Copy className="size-4" />复制转录结果</DropdownMenu.Item>
                <DropdownMenu.Item className={menuItem} disabled={entry.deleting || !entry.polished_text?.trim()} onSelect={() => { void action(entry, 'polished') }}><Copy className="size-4" />复制润色结果</DropdownMenu.Item>
                <DropdownMenu.Separator className="my-1 h-px bg-border" />
                <DropdownMenu.Item className={`${menuItem} text-destructive focus:bg-destructive/10`} onSelect={() => { openingDialog.current = true; setError(''); setNotice(''); setDeleting(entry) }}><Trash2 className="size-4" />{entry.deleting ? '重试删除' : '删除'}</DropdownMenu.Item>
              </DropdownMenu.Content>
            </DropdownMenu.Portal>
          </DropdownMenu.Root>
        </div>
      </div>)}
    </div>
    {page?.next_cursor && <Button variant="outline" className="w-full" disabled={loading} onClick={() => load(page.next_cursor!)}>{loading ? '加载中…' : '加载更多'}</Button>}
    <p role="status" className="min-h-4 text-xs text-muted-foreground">{notice}</p>
    {(error || guard.error) && !deleting && <p role="alert" className="text-sm text-destructive">{error || guard.error}</p>}
    <Dialog open={deleting !== null} onOpenChange={open => { if (!open) setDeleting(null) }} title="删除这条记录？" description="此操作不可恢复。" busy={saving}
      onCloseAutoFocus={event => { event.preventDefault(); if (opener.current?.isConnected) opener.current.focus(); else title.current?.focus() }}>
      {(error || guard.error) && <p role="alert" className="mt-3 text-xs text-destructive">{error || guard.error}</p>}
      <div className="mt-5 flex justify-end gap-2">
        <Button data-dialog-autofocus variant="outline" disabled={saving} onClick={() => setDeleting(null)}>取消</Button>
        <Button variant="destructive" disabled={saving || !guard.ready} onClick={() => { void remove() }}>{saving ? '删除中…' : '删除'}</Button>
      </div>
    </Dialog>
  </section>
}
