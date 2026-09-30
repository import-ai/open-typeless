import { test } from 'node:test'
import assert from 'node:assert/strict'
import { calendarWeeks, dateLabel, durationLabel, recordingTime, usageLevel, previewHistory, previewInsights, deletePreviewEntry } from '../frontend/src/lib/history.ts'

test('calendar covers six calendar months without duplicating dates across DST, leap days, or years', () => {
  const previous = process.env.TZ
  try {
    for (const timezone of ['Asia/Shanghai', 'America/New_York']) {
      process.env.TZ = timezone
      for (const [today, start, length] of [['2024-03-31', '2023-10-01', 183], ['2026-10-01', '2026-05-01', 154], ['2026-01-01', '2025-08-01', 154]]) {
        const weeks = calendarWeeks(today)
        const days = weeks.flat().filter(Boolean)
        assert.equal(days[0], start)
        assert.equal(days.at(-1), today)
        assert.equal(days.length, length)
        assert.equal(new Set(days).size, length)
        assert.ok(weeks.every(week => week.length === 7))
        for (const week of weeks) if (week[0]) assert.equal(new Date(`${week[0]}T12:00:00`).getDay(), 1)
      }
    }
  } finally {
    if (previous === undefined) delete process.env.TZ
    else process.env.TZ = previous
  }
})

test('labels preserve recorded local time and handle midnight, negative savings, and intensity boundaries', () => {
  assert.equal(dateLabel('2025-12-31', '2026-01-01'), '昨天')
  assert.equal(dateLabel('2026-01-01', '2026-01-01'), '今天')
  assert.equal(dateLabel('2025-12-30', '2026-01-01'), '2025-12-30')
  assert.equal(recordingTime({ started_at_ms: Date.parse('2026-09-30T16:01:02Z'), utc_offset_minutes: 480 }), '00:01:02')
  assert.equal(durationLabel(300 * 2000 - 120_000), '8 分钟')
  assert.equal(durationLabel(-120_000), '−2 分钟')
  assert.equal(durationLabel(0), '0 秒')
  assert.deepEqual([0, 1, 99, 100, 499, 500, 999, 1000].map(usageLevel), [0, 1, 1, 2, 2, 3, 3, 4])
})

test('preview deletion changes history without changing usage', () => {
  const before = previewHistory()
  const usage = previewInsights()
  deletePreviewEntry(before.entries[0].id)
  assert.equal(previewHistory().entries.length, before.entries.length - 1)
  assert.deepEqual(previewInsights(), usage)
})
