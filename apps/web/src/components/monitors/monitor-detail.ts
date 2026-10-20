import type { Heartbeat, HeartbeatStatus, LatencyPoint, ProbeState } from '~/generated'

/** A status change in a run of beats: the first beat with the new status. */
export interface StatusChange {
  beat: Heartbeat
  /** How long the status before it lasted, when its start is among the beats. */
  after?: number
}

/**
 * Status changes in beats given newest first, newest change first. Each probe's beats are their
 * own run, so probes that disagree do not read as flapping. The oldest beat of a run is not a
 * change: what came before it is unknown.
 */
export function statusChanges(beats: readonly Heartbeat[]): StatusChange[] {
  const runs = new Map<string, Heartbeat[]>()
  for (const beat of beats) {
    const key = beat.probe ?? ''
    const run = runs.get(key)
    if (run) run.push(beat)
    else runs.set(key, [beat])
  }
  const changes: StatusChange[] = []
  for (const run of runs.values()) {
    let previous: Heartbeat | undefined
    let since: number | undefined
    for (const beat of [...run].reverse()) {
      if (previous && beat.status !== previous.status) {
        changes.push({ beat, after: since === undefined ? undefined : beat.time - since })
        since = beat.time
      }
      previous = beat
    }
  }
  return changes.sort((a, b) => b.beat.time - a.beat.time)
}

/**
 * Average, 95th percentile and highest response time over a series of buckets. Buckets carry no
 * beat counts, so the average and percentile are over bucket averages. `null` without buckets.
 */
export function latencyStats(points: readonly LatencyPoint[]): { avg: number; p95: number; max: number } | null {
  if (points.length === 0) return null
  const averages = points.map((point) => point.avg_ms).sort((a, b) => a - b)
  const avg = averages.reduce((sum, value) => sum + value, 0) / averages.length
  const p95 = averages[Math.min(averages.length - 1, Math.ceil(averages.length * 0.95) - 1)] ?? 0
  const max = Math.max(...points.map((point) => point.max_ms))
  return { avg, p95, max }
}

const spanUnits = [
  ['day', 86_400_000],
  ['hour', 3_600_000],
  ['minute', 60_000],
  ['second', 1000],
] as const

/** A duration in its largest whole unit, "14 min" or "3 hr"; under a second reads as seconds. */
export function formatSpan(ms: number, locale: string): string {
  const [unit, size] = spanUnits.find(([, size]) => ms >= size) ?? spanUnits[3]
  return new Intl.NumberFormat(locale, { style: 'unit', unit, unitDisplay: 'short' }).format(Math.floor(ms / size))
}

/** How many live probes see `status`, out of those that count in the quorum. */
export function quorumCount(states: readonly ProbeState[], status: HeartbeatStatus): { count: number; total: number } {
  const live = states.filter((state) => state.live)
  return { count: live.filter((state) => state.status === status).length, total: live.length }
}
