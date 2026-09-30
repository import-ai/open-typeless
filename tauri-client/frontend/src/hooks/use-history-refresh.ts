import { useEffect, useState } from 'react'
import { useTauriEvent } from '@/hooks/use-tauri-event'
import { localDate } from '@/lib/history'

export function useHistoryRefresh(reload: () => void): string {
  const [today, setToday] = useState(() => localDate(new Date()))
  useTauriEvent('history-changed', reload)
  useEffect(() => {
    reload()
    window.addEventListener('focus', reload)
    return () => window.removeEventListener('focus', reload)
  }, [reload, today])
  useEffect(() => {
    const timer = setInterval(() => setToday(localDate(new Date())), 60_000)
    return () => clearInterval(timer)
  }, [])
  return today
}
