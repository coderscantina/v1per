import { describe, expect, it } from 'vitest'

import type { Heartbeat, HeartbeatStatus, ProbeState } from '~/generated'

import { formatSpan, latencyStats, quorumCount, statusChanges } from './monitor-detail'

const beat = (time: number, status: HeartbeatStatus, probe?: string): Heartbeat => ({ time, status, probe, message: '', retries: 0 })

describe('monitor detail', () => {
  it('finds status changes per probe, newest first, with how long the status before lasted', () => {
    const beats = [beat(6, 'up'), beat(5, 'down', 'b'), beat(4, 'down'), beat(3, 'up', 'b'), beat(2, 'up'), beat(1, 'down')]
    expect(statusChanges(beats).map(({ beat, after }) => [beat.time, beat.status, after])).toEqual([
      [6, 'up', 2],
      [5, 'down', undefined],
      [4, 'down', 2],
      [2, 'up', undefined],
    ])
  })

  it('sums up response times over bucket averages and the highest bucket', () => {
    const points = [10, 20, 30, 40].map((avg, index) => ({ bucket: index, min_ms: avg, avg_ms: avg, max_ms: avg * 2 }))
    expect(latencyStats(points)).toEqual({ avg: 25, p95: 40, max: 80 })
    expect(latencyStats([])).toBeNull()
  })

  it('reads a duration in its largest whole unit', () => {
    expect(formatSpan(14 * 60_000 + 30_000, 'en')).toBe('14 min')
    expect(formatSpan(26 * 3_600_000, 'en')).toBe('1 day')
    expect(formatSpan(400, 'en')).toBe('0 sec')
  })

  it('counts the live probes that see a status', () => {
    const state = (status: HeartbeatStatus, live: boolean) => ({ status, live }) as ProbeState
    expect(quorumCount([state('down', true), state('down', true), state('up', true), state('down', false)], 'down')).toEqual({
      count: 2,
      total: 3,
    })
  })
})
