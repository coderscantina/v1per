import { describe, expect, it, vi } from 'vitest'
import { ref } from 'vue'

import type { SlaPeriod } from '~/generated'

import { useSlaFormat } from './useSlaFormat'

vi.mock('./usePreferences', () => ({ useFormats: () => ({ numberLocale: ref('en') }) }))
vi.mock('./useDateFormat', () => ({ useDateFormat: () => ({ date: () => '' }) }))

const DAY = 86_400_000

function period(extra: Partial<SlaPeriod> = {}): SlaPeriod {
  return {
    sla: '7',
    start: '2026-10-01',
    starts_at: Date.now() - DAY,
    ends_at: Date.now() + DAY,
    terms: {
      name: 'Acme hosting',
      timezone: 'Europe/Vienna',
      period: 'month',
      start_date: '2026-01-01',
      combine: 'each',
      latency_ms: 250,
      times: { major_outage: { response_minutes: 15, resolution_minutes: 240 } },
      monitors: ['11'],
    },
    monitors: [],
    incidents: [],
    ...extra,
  }
}

describe('sla durations', () => {
  it('writes spans as the SLA list does and short ones as under a minute', () => {
    const { duration, minutes } = useSlaFormat()
    expect(minutes(15)).toBe('15 min')
    expect(minutes(240)).toBe('4 h')
    expect(duration(90 * 60_000)).toBe('1 h 30 min')
    expect(duration(59_999)).toBe('under 1 min')
    expect(duration(60_000)).toBe('1 min')
  })

  it('uses the same format for time targets and averages', () => {
    const { commitments } = useSlaFormat()
    const rows = commitments(
      period({
        response: { outcome: 'met', counted: 1, met: 1, average_ms: 30_000 },
        resolution: { outcome: 'met', counted: 1, met: 1, average_ms: 3_600_000 },
      })
    )
    expect(rows.find((row) => row.key === 'response')).toMatchObject({ target: 'Major outage 15 min', notes: ['1 of 1 within target', 'Average under 1 min'] })
    expect(rows.find((row) => row.key === 'resolution')).toMatchObject({ target: 'Major outage 4 h', notes: ['1 of 1 within target', 'Average 1 h'] })
  })
})

describe('sla latency caption', () => {
  it('says a running period judges latency when it ends, and a closed one does not', () => {
    const { commitments } = useSlaFormat()
    const running = commitments(period({ latency: 'at_risk' })).find((row) => row.key === 'latency')
    expect(running?.notes).toContain('The target is judged when the period ends.')
    const closed = commitments(period({ latency: 'met', ends_at: Date.now() - 1 })).find((row) => row.key === 'latency')
    expect(closed?.notes).not.toContain('The target is judged when the period ends.')
  })
})
