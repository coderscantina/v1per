import type { Resolution, StatusBucket } from '~/generated'

/**
 * The time frames of the monitor page. Each reads rollups at a resolution that stays under the
 * point limit and draws `slots` bars of `slot` milliseconds each, aligned in UTC.
 */
export const historyRanges = {
  '1h': { resolution: 'minute', slot: 60_000, slots: 60 },
  '24h': { resolution: 'minute', slot: 15 * 60_000, slots: 96 },
  '7d': { resolution: 'hour', slot: 2 * 3_600_000, slots: 84 },
  '30d': { resolution: 'hour', slot: 8 * 3_600_000, slots: 90 },
  '90d': { resolution: 'day', slot: 86_400_000, slots: 90 },
} as const satisfies Record<string, { resolution: Resolution; slot: number; slots: number }>

export type HistoryRange = keyof typeof historyRanges
export const historyRangeNames = ['1h', '24h', '7d', '30d', '90d'] as const satisfies readonly HistoryRange[]
export const isHistoryRange = (value: unknown): value is HistoryRange => historyRangeNames.some((name) => name === value)

/** What one read covers: from the first slot's start to the end of the slot holding `now`. */
export interface HistoryWindow {
  range: HistoryRange
  from: number
  to: number
  now: number
}

export function historyWindow(range: HistoryRange, now: number): HistoryWindow {
  const { slot, slots } = historyRanges[range]
  const to = Math.floor(now / slot) * slot + slot
  return { range, from: to - slot * slots, to, now }
}

/** One bar: the beats of its buckets summed, with uptime as `up / (up + down)`. */
export interface HistorySlot {
  start: number
  end: number
  up: number
  down: number
  maintenance: number
  /** Absent without up or down beats. */
  uptime?: number
}

const uptimeOf = (up: number, down: number) => (up + down > 0 ? up / (up + down) : undefined)

/** The buckets of `window` summed into its bars, oldest first; bars without beats are empty. */
export function historySlots(buckets: readonly StatusBucket[], window: HistoryWindow): HistorySlot[] {
  const { slot, slots } = historyRanges[window.range]
  const list: HistorySlot[] = Array.from({ length: slots }, (_, index) => {
    const start = window.from + index * slot
    return { start, end: start + slot, up: 0, down: 0, maintenance: 0 }
  })
  for (const bucket of buckets) {
    const entry = list[Math.floor((bucket.bucket - window.from) / slot)]
    if (!entry) continue
    entry.up += bucket.up
    entry.down += bucket.down
    entry.maintenance += bucket.maintenance
  }
  return list.map((entry) => ({ ...entry, uptime: uptimeOf(entry.up, entry.down) }))
}

/** Uptime over every bucket, or `undefined` without up or down beats. */
export function totalUptime(buckets: readonly StatusBucket[]): number | undefined {
  let up = 0
  let down = 0
  for (const bucket of buckets) {
    up += bucket.up
    down += bucket.down
  }
  return uptimeOf(up, down)
}
