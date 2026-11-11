import { describe, expect, it } from 'vitest'

import type { LatencyPoint, ResponseSeries } from '~/generated'

import { bandPath, linePath, markSpan, nearest, pickSeries, runs, scaleTop, snapshotData, type Frame } from './response-chart'

const HOUR = 3_600_000
const point = (hour: number, avg: number, min = avg, max = avg): LatencyPoint => ({ bucket: hour * HOUR, avg_ms: avg, min_ms: min, max_ms: max })
const frame: Frame = { from: 0, to: 10 * HOUR, step: HOUR, top: 100 }

describe('response chart', () => {
  it('keeps the six slowest series in the server order with stable color slots', () => {
    const series: ResponseSeries[] = [10, 80, undefined, 40, 30, 20, 70, 60].map((avg_ms, index) => ({ label: `s${index}`, points: [], avg_ms }))
    const { shown, hidden } = pickSeries(series)
    expect(shown.map((entry) => entry.label)).toEqual(['s1', 's3', 's4', 's5', 's6', 's7'])
    expect(shown.map((entry) => entry.slot)).toEqual([1, 2, 3, 4, 5, 6])
    expect(hidden).toBe(2)
  })

  it('reads the snapshot points as the last 24 hours, or 2 hours per minute for a new monitor', () => {
    const hourly = snapshotData([point(5, 10), point(4, 12)], 5.5 * HOUR)
    expect([hourly.step, hourly.from, hourly.to]).toEqual([HOUR, -18 * HOUR, 6 * HOUR])
    const minutes = snapshotData([{ ...point(0, 5), bucket: 60_000 }, { ...point(0, 5), bucket: 120_000 }], 130_000)
    expect([minutes.step, minutes.to - minutes.from]).toEqual([60_000, 2 * HOUR])
  })

  it('cuts the scale at twice the highest average so one outlier does not flatten the lines', () => {
    expect(scaleTop([[point(0, 100, 90, 120)]])).toBe(200)
    expect(scaleTop([[point(0, 100, 90, 5000)]])).toBe(300)
    expect(scaleTop([])).toBe(2)
  })

  it('breaks lines at gaps and draws a lone point as a level line', () => {
    const points = [point(0, 50), point(1, 50), point(5, 100, 50, 100)]
    expect(runs(points, HOUR).map((run) => run.length)).toEqual([2, 1])
    expect(linePath(frame, points)).toBe('M5.00,50.00L15.00,50.00M50.00,0.00L60.00,0.00')
    expect(bandPath(frame, [point(5, 75, 50, 100)])).toBe('M50.00,0.00L60.00,0.00L60.00,50.00L50.00,50.00Z')
  })

  it('places marks within the frame and runs an ongoing one to the end', () => {
    expect(markSpan(frame, { kind: 'incident', title: 'Down', from: -HOUR, to: 2 * HOUR })).toEqual({ start: 0, width: 20 })
    expect(markSpan(frame, { kind: 'maintenance', title: 'Upgrade', from: 9 * HOUR })).toEqual({ start: 90, width: 10 })
  })

  it('finds the point under the pointer', () => {
    const points = [point(0, 1), point(4, 2), point(8, 3)]
    expect(nearest(frame, points, 5 * HOUR)?.avg_ms).toBe(2)
    expect(nearest(frame, [], 5 * HOUR)).toBeUndefined()
  })
})
