import { describe, expect, it } from 'vitest'

import { historySlots, historyWindow, totalUptime } from './monitor-history'

const HOUR = 3_600_000

describe('monitor history', () => {
  it('ends the window with the slot holding now', () => {
    const now = Date.UTC(2026, 9, 20, 10, 30)
    const window = historyWindow('7d', now)
    expect(window.to).toBe(Date.UTC(2026, 9, 20, 12))
    expect(window.to - window.from).toBe(84 * 2 * HOUR)
  })

  it('sums buckets into their slots and leaves slots without beats empty', () => {
    const window = historyWindow('7d', Date.UTC(2026, 9, 20, 10, 30))
    const at = (offset: number) => window.from + offset * HOUR
    const slots = historySlots(
      [
        { bucket: at(0), up: 3, down: 1, maintenance: 0 },
        { bucket: at(1), up: 4, down: 0, maintenance: 2 },
        { bucket: at(-1), up: 9, down: 9, maintenance: 0 },
      ],
      window
    )
    expect(slots).toHaveLength(84)
    expect(slots[0]).toMatchObject({ up: 7, down: 1, maintenance: 2, uptime: 7 / 8 })
    expect(slots[1]?.uptime).toBeUndefined()
  })

  it('takes uptime over every bucket', () => {
    expect(totalUptime([{ bucket: 0, up: 3, down: 1, maintenance: 5 }])).toBe(0.75)
    expect(totalUptime([{ bucket: 0, up: 0, down: 0, maintenance: 5 }])).toBeUndefined()
  })
})
