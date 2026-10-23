import { describe, expect, it } from 'vitest'

import type { PageSla, PageSlaPeriod } from '~/generated'

import { formatMinutes, formatPeriod, periodLines, slaPeriodView, slaView } from './sla'

function period(extra: Partial<PageSlaPeriod> = {}): PageSlaPeriod {
  return { start: '2026-10-01', starts_at: 0, ends_at: 1, outcome: 'on_track', ...extra }
}

const names = new Map([['c1', 'API']])

describe('slaPeriodView', () => {
  it('reads a running period at risk from 75 % of the budget or a latency above target', () => {
    const budget = slaPeriodView(period({ availability: { target: 99.9, outcome: 'on_track', actual: 99.93, budget_used: 0.75 } }), names)
    expect([budget.state, budget.availability?.state]).toEqual(['at_risk', 'at_risk'])

    const calm = slaPeriodView(period({ availability: { target: 99.9, outcome: 'on_track', actual: 99.99, budget_used: 0.1 } }), names)
    expect(calm.state).toBe('on_track')

    const slow = slaPeriodView(period({ latency: { target_ms: 300, outcome: 'on_track', actual_ms: 340 } }), names)
    expect([slow.state, slow.latency?.state]).toEqual(['at_risk', 'at_risk'])
  })

  it('keeps a miss and no data as the server says, never turning no data into a miss', () => {
    const missed = slaPeriodView(
      period({ outcome: 'missed', availability: { target: 99.9, outcome: 'missed', actual: 99, budget_used: 10 } }),
      names
    )
    expect(missed.state).toBe('missed')

    const empty = slaPeriodView(period({ outcome: 'no_data', availability: { target: 99.9, outcome: 'no_data' } }), names)
    expect([empty.state, empty.availability?.state]).toEqual(['no_data', 'no_data'])
    expect(periodLines(empty, 'en').map((line) => line.text)).toEqual(['Availability: no data, target 99.9%'])
  })

  it('orders time targets worst impact first and names monitors by their component', () => {
    const view = slaPeriodView(
      period({
        response: { targets: { degraded: 240, major_outage: 15 }, outcome: 'on_track', counted: 2, met: 2, within_target: 1, average_ms: 600_000 },
        monitors: [
          { component: 'c1', availability: 99.95, availability_outcome: 'on_track' },
          { component: 'gone', availability: 50 },
        ],
      }),
      names
    )
    expect(view.response?.targets).toEqual([
      { impact: 'major_outage', minutes: 15 },
      { impact: 'degraded', minutes: 240 },
    ])
    expect(view.monitors.map((monitor) => monitor.name)).toEqual(['API'])
  })
})

describe('slaView', () => {
  it('lists closed periods oldest first', () => {
    const sla: PageSla = {
      name: 'Hosting',
      timezone: 'Europe/Vienna',
      period: 'month',
      combine: 'any_down',
      history: [period({ start: '2026-09-01', outcome: 'met' }), period({ start: '2026-08-01', outcome: 'missed' })],
    }
    expect(slaView(sla, names).history.map((closed) => [closed.start, closed.state])).toEqual([
      ['2026-08-01', 'missed'],
      ['2026-09-01', 'met'],
    ])
  })
})

describe('formatting', () => {
  it('names periods and whole-hour targets', () => {
    expect(formatPeriod('2026-10-01', 'month', 'en')).toBe('October 2026')
    expect(formatPeriod('2026-10-01', 'quarter', 'en')).toBe('Q4 2026')
    expect(formatPeriod('2026-01-01', 'year', 'en')).toBe('2026')
    expect(formatMinutes(240)).toBe('4 h')
    expect(formatMinutes(90)).toBe('1 h 30 min')
    expect(formatMinutes(2880)).toBe('2 d')
  })
})
