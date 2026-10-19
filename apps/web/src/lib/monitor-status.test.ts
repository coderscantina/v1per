import { describe, expect, it } from 'vitest'

import type { DayBar } from '~/generated'

import { barTone, countPresences, formatLatency, formatUptime, presenceOf } from './monitor-status'

const bar = (uptime: number | undefined, maintenance = 0): DayBar => ({
  day: '2026-10-01',
  up: 0,
  down: 0,
  pending: 0,
  maintenance,
  uptime,
  utc_approximate: false,
})

describe('monitor status presentation', () => {
  it('reads paused before any status and unknown before the first beat', () => {
    expect(presenceOf({ active: false, status: 'down' })).toBe('paused')
    expect(presenceOf({ active: true })).toBe('unknown')
    expect(presenceOf({ active: true, status: 'pending' })).toBe('pending')
    expect(
      countPresences([{ active: true, status: 'up' }, { active: true, status: 'up' }, { active: false, status: 'up' }, { active: true }])
    ).toEqual({ down: 0, pending: 0, maintenance: 0, up: 2, unknown: 1, paused: 1 })
  })

  it('shows uptime with two decimals and never rounds a miss up to 100 %', () => {
    expect(formatUptime(0.99999, 'en')).toBe('99.99%')
    expect(formatUptime(1, 'en')).toBe('100%')
    expect(formatUptime(0.5, 'de')).toBe('50,00\u00a0%')
    expect(formatUptime(undefined, 'en')).toBeNull()
    expect([formatLatency(1.46, 'en'), formatLatency(842.6, 'en')]).toEqual(['1.5 ms', '843 ms'])
  })

  it('colours a day by its uptime, maintenance-only and empty days apart', () => {
    expect([bar(1), bar(0.97), bar(0.9), bar(undefined, 4), bar(undefined)].map(barTone)).toEqual([
      'up',
      'degraded',
      'down',
      'maintenance',
      'empty',
    ])
  })
})
