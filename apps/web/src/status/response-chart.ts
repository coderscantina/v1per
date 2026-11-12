import type { LatencyPoint, PageResponseTimes, ResponseMark, ResponseRange, ResponseSeries } from '~/generated'

/**
 * The math of the response time chart on the status page, kept apart from the component so it
 * can be tested: which series to draw, the scale, the paths in a 0 to 100 box, and the spans of
 * incidents and maintenance.
 */

export const ranges: readonly ResponseRange[] = ['24h', '7d', '30d', '90d']

/** The most series drawn at once; the slowest win. */
export const SERIES_MAX = 6

const HOUR_MS = 3_600_000

/** A series as the chart draws it, with the color slot (1 to 6) it keeps. */
export interface ChartSeries {
  label: string
  slot: number
  series: ResponseSeries
}

/** What the chart draws for one range. */
export interface ChartData {
  range: ResponseRange
  from: number
  to: number
  step: number
  overall: ResponseSeries
  series: ChartSeries[]
  /** Series left out because more than six exist. */
  hidden: number
  marks: ResponseMark[]
}

/**
 * The six slowest series by average, in the server's order, so a series keeps its color while
 * the others come and go. Series without an average count as fastest.
 */
export function pickSeries(series: readonly ResponseSeries[]): { shown: ChartSeries[]; hidden: number } {
  const slowest = new Set(
    [...series]
      .sort((a, b) => (b.avg_ms ?? -1) - (a.avg_ms ?? -1))
      .slice(0, SERIES_MAX)
  )
  const shown = series.filter((entry) => slowest.has(entry)).map((entry, index) => ({ label: entry.label ?? '', slot: index + 1, series: entry }))
  return { shown, hidden: series.length - shown.length }
}

/** An answer of `response-times.json` as chart data. */
export function chartData(answer: PageResponseTimes): ChartData {
  const { shown, hidden } = pickSeries(answer.series)
  return { range: answer.range, from: answer.from, to: answer.to, step: answer.step, overall: answer.overall, series: shown, hidden, marks: answer.marks }
}

/**
 * The snapshot's 24-hour points as chart data until the full answer arrives: hourly, or per
 * minute over 2 hours for a new monitor, ending with the bucket that holds `now`.
 */
export function snapshotData(points: readonly LatencyPoint[], now: number): ChartData {
  const sorted = [...points].sort((a, b) => a.bucket - b.bucket)
  const gaps = sorted.slice(1).map((point, index) => point.bucket - (sorted[index]?.bucket ?? 0))
  const step = gaps.length > 0 ? Math.min(...gaps) : HOUR_MS
  const to = Math.floor(now / step) * step + step
  const from = to - (step < HOUR_MS ? 2 * HOUR_MS : 24 * HOUR_MS)
  return { range: '24h', from, to, step, overall: { points: sorted }, series: [], hidden: 0, marks: [] }
}

/** The next round number at or above a positive value: one significant digit. */
function roundUp(value: number): number {
  const magnitude = 10 ** Math.floor(Math.log10(value))
  return Math.ceil(value / magnitude) * magnitude
}

/**
 * The top of the scale over the drawn series. One slow outlier would flatten the lines, so a
 * band far above the averages is cut at twice the highest average.
 */
export function scaleTop(series: readonly (readonly LatencyPoint[])[]): number {
  const points = series.flat()
  const highestAverage = Math.max(1, ...points.map((point) => point.avg_ms))
  const highest = Math.max(1, ...points.map((point) => point.max_ms))
  return roundUp(Math.min(highest, highestAverage * 2) * 1.05)
}

/** Where a chart's box maps time and milliseconds: 0 to 100 across and down. */
export interface Frame {
  from: number
  to: number
  step: number
  top: number
}

const clamp = (value: number) => Math.min(100, Math.max(0, value))

export function left(frame: Frame, at: number): number {
  return clamp(((at - frame.from) / Math.max(1, frame.to - frame.from)) * 100)
}

export function down(frame: Frame, ms: number): number {
  return 100 - (Math.min(Math.max(0, ms), frame.top) / frame.top) * 100
}

/** The middle of a point's bucket, where it is drawn. */
export const middle = (frame: Frame, point: LatencyPoint) => point.bucket + frame.step / 2

/** Runs of points without a gap longer than one and a half steps. */
export function runs(points: readonly LatencyPoint[], step: number): LatencyPoint[][] {
  const list: LatencyPoint[][] = []
  for (const point of points) {
    const run = list.at(-1)
    const last = run?.at(-1)
    if (run && last && point.bucket - last.bucket <= step * 1.5) run.push(point)
    else list.push([point])
  }
  return list
}

const at = (frame: Frame, x: number, ms: number) => `${left(frame, x).toFixed(2)},${down(frame, ms).toFixed(2)}`

/** The edges of a run along time: bucket middles, or the bucket's width for a lone point, so one point draws a level line. */
function edges(frame: Frame, run: readonly LatencyPoint[]): { point: LatencyPoint; x: number }[] {
  const only = run[0]
  if (run.length === 1 && only) return [{ point: only, x: only.bucket }, { point: only, x: only.bucket + frame.step }]
  return run.map((point) => ({ point, x: middle(frame, point) }))
}

/** The average line of `points`, broken at gaps. */
export function linePath(frame: Frame, points: readonly LatencyPoint[]): string {
  return runs(points, frame.step)
    .map((run) =>
      edges(frame, run)
        .map(({ point, x }, index) => `${index === 0 ? 'M' : 'L'}${at(frame, x, point.avg_ms)}`)
        .join('')
    )
    .join('')
}

/** The band from the fastest to the slowest check of each bucket, broken at gaps. */
export function bandPath(frame: Frame, points: readonly LatencyPoint[]): string {
  return runs(points, frame.step)
    .map((run) => {
      const line = edges(frame, run)
      const upper = line.map(({ point, x }, index) => `${index === 0 ? 'M' : 'L'}${at(frame, x, point.max_ms)}`)
      const lower = line.toReversed().map(({ point, x }) => `L${at(frame, x, point.min_ms)}`)
      return `${upper.join('')}${lower.join('')}Z`
    })
    .join('')
}

/** A mark's span in percent of the width; an ongoing one runs to the end. */
export function markSpan(frame: Frame, mark: ResponseMark): { start: number; width: number } {
  const start = left(frame, mark.from)
  return { start, width: left(frame, mark.to ?? frame.to) - start }
}

/** The point whose bucket middle is closest to `at`. */
export function nearest(frame: Frame, points: readonly LatencyPoint[], at: number): LatencyPoint | undefined {
  let best: LatencyPoint | undefined
  for (const point of points) {
    if (!best || Math.abs(middle(frame, point) - at) < Math.abs(middle(frame, best) - at)) best = point
  }
  return best
}
