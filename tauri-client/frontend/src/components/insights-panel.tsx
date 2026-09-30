import { useCallback, useEffect, useRef, useState } from 'react'
import { Card, CardContent } from '@/components/ui/card'
import { Button } from '@/components/ui/button'
import { useHistoryRefresh } from '@/hooks/use-history-refresh'
import { commands } from '@/lib/desktop'
import { calendarWeeks, durationLabel, usageLevel, type Insights } from '@/lib/history'

const shades = ['bg-zinc-100', 'bg-zinc-300', 'bg-zinc-400', 'bg-zinc-600', 'bg-zinc-900']

export function InsightsPanel() {
  const [data, setData] = useState<Insights | null>(null)
  const [error, setError] = useState('')
  const [detail, setDetail] = useState('')
  const request = useRef(0)
  useEffect(() => () => { request.current++ }, [])
  const reload = useCallback(() => {
    const current = ++request.current
    void commands.insights().then(value => {
      if (request.current === current) { setData(value); setError('') }
    }).catch(error => { if (request.current === current) setError(String(error)) })
  }, [])
  const today = useHistoryRefresh(reload)
  const weeks = calendarWeeks(today)
  const days = new Map(data?.days.map(day => [day.local_date, day]))
  const speed = data?.audio_duration_ms ? Math.round(data.character_count * 60_000 / data.audio_duration_ms) : null
  const stats = data ? [
    ['节省时间', durationLabel(data.character_count * 2000 - data.audio_duration_ms)],
    ['口述字数', `${data.character_count.toLocaleString()} 字`],
    ['口述时长', durationLabel(data.audio_duration_ms)],
    ['口述速度', speed === null ? '—' : `${speed} 字/分钟`],
  ] : []

  return <section className="space-y-3" aria-labelledby="insights-title">
    <div className="flex items-center justify-between">
      <h2 id="insights-title" className="text-base font-semibold">洞察</h2>
      <span className="text-xs text-muted-foreground">累计统计</span>
    </div>
    {error ? <div role="alert" className="space-y-2 text-sm text-destructive"><p>{error}</p><Button variant="outline" onClick={reload}>重新加载</Button></div>
      : !data ? <p role="status" className="text-sm text-muted-foreground">正在加载洞察…</p>
      : <Card><CardContent className="space-y-4">
        <dl className="grid grid-cols-2 gap-x-4 gap-y-5">
          {stats.map(([label, value]) => <div key={label}>
            <dt className="text-xs text-muted-foreground">{label}</dt>
            <dd className="mt-1 text-lg font-semibold tabular-nums tracking-tight">{value}</dd>
          </div>)}
        </dl>
        <p className="text-[10px] text-muted-foreground">节省时间按打字速度 30 字/分钟估算</p>
        <div className="space-y-3 border-t pt-4">
          <div className="flex items-center justify-between text-xs"><span className="font-medium">使用强度</span><span className="text-muted-foreground">最近 6 个月</span></div>
          <div className="flex gap-1.5">
            <div className="grid shrink-0 grid-rows-[16px_repeat(7,10px)] gap-[3px] text-[9px] leading-[10px] text-muted-foreground" aria-hidden="true">
              {['', '一', '', '三', '', '五', '', ''].map((label, index) => <span key={index}>{label}</span>)}
            </div>
            <div className="grid min-w-0 flex-1 gap-[3px]" style={{ gridTemplateColumns: `repeat(${weeks.length}, minmax(0, 1fr))` }}>
              {weeks.map((week, index) => {
                const first = week.find(day => day !== null)
                const month = week.find(day => day?.endsWith('-01')) ?? (index === 0 ? first : null)
                return <div key={index} className="grid min-w-0 grid-cols-[minmax(0,1fr)] grid-rows-[16px_repeat(7,10px)] gap-[3px]">
                  <span className="overflow-visible whitespace-nowrap text-[9px] text-muted-foreground">{month ? `${Number(month.slice(5, 7))}月` : ''}</span>
                  {week.map((date, dayIndex) => {
                    const usage = date ? days.get(date) : undefined
                    const label = `${date} · ${(usage?.character_count ?? 0).toLocaleString()} 字 · ${durationLabel(usage?.audio_duration_ms ?? 0)} · ${usage?.recognition_count ?? 0} 次`
                    return date ? <button key={date} aria-label={label} title={label}
                      onFocus={() => setDetail(label)} onBlur={() => setDetail('')} onMouseEnter={() => setDetail(label)} onMouseLeave={() => setDetail('')}
                      className={`h-[10px] min-w-0 rounded-[2px] outline-none focus-visible:ring-2 focus-visible:ring-ring ${shades[usageLevel(usage?.character_count ?? 0)]}`} />
                      : <span key={dayIndex} />
                  })}
                </div>
              })}
            </div>
          </div>
          <div className="flex items-center justify-end gap-1 text-[10px] text-muted-foreground" aria-label="使用强度从少到多：0、1 至 99、100 至 499、500 至 999、1000 字及以上">
            <span>少</span>{shades.map(shade => <span key={shade} className={`size-2.5 rounded-[2px] ${shade}`} />)}<span>多</span>
          </div>
          <p role="status" className="min-h-4 text-[10px] text-muted-foreground">{detail || '每格代表一天，颜色越深表示口述字数越多。'}</p>
        </div>
      </CardContent></Card>}
  </section>
}
