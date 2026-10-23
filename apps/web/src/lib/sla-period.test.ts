import { describe, expect, it } from 'vitest'

import type { SlaPeriod } from '~/generated'

import { csvFileName, highestLatency, periodStarts, periodTitle, slaPeriodCsv, todayIn, toCsv } from './sla-period'

const period: SlaPeriod = {
  sla: '7',
  start: '2026-10-01',
  starts_at: Date.UTC(2026, 8, 30, 22),
  ends_at: Date.UTC(2026, 9, 31, 23),
  terms: {
    name: 'Acme hosting',
    timezone: 'Europe/Vienna',
    period: 'month',
    start_date: '2026-01-15',
    combine: 'each',
    availability: 99.9,
    monitors: ['11', '12'],
  },
  monitors: [
    { monitor: '11', name: 'Shop, "main"', availability: 99.95123, availability_outcome: 'met', budget_used: 0.4876, latency_ms: 212.34 },
    { monitor: '12', name: '=cmd', availability_outcome: 'no_data' },
  ],
  incidents: [
    {
      incident: '5',
      title: 'Checkout down\nagain',
      started_at: Date.UTC(2026, 9, 3, 8),
      resolved_at: Date.UTC(2026, 9, 3, 9, 30),
      impact: 'major_outage',
      response: { outcome: 'met', elapsed_ms: 750_000 },
      resolution: { outcome: 'missed', elapsed_ms: 5_400_000, deadline: Date.UTC(2026, 9, 3, 9) },
    },
  ],
}

describe('sla periods', () => {
  it('lists periods from the start date through today, newest first', () => {
    expect(periodStarts('month', '2026-08-15', '2026-10-23')).toEqual(['2026-10-01', '2026-09-01', '2026-08-15'])
    expect(periodStarts('quarter', '2025-11-02', '2026-04-01')).toEqual(['2026-04-01', '2026-01-01', '2025-11-02'])
    expect(periodStarts('year', '2026-01-01', '2026-12-31')).toEqual(['2026-01-01'])
    expect(periodStarts('month', '2026-12-01', '2026-10-23')).toEqual(['2026-12-01'])
  })

  it('names periods and reads today in a zone', () => {
    expect(periodTitle('month', '2026-08-15', 'en')).toBe('August 2026')
    expect(periodTitle('quarter', '2026-11-02', 'de')).toBe('Q4 2026')
    expect(periodTitle('year', '2026-03-01', 'en')).toBe('2026')
    expect(todayIn('Pacific/Auckland', Date.UTC(2026, 9, 23, 12))).toBe('2026-10-24')
  })

  it('takes the highest average latency', () => {
    expect(highestLatency(period)).toBe(212.34)
    expect(highestLatency({ monitors: [] })).toBeNull()
  })
})

describe('csv', () => {
  it('quotes separators, quotes and line breaks and defuses formulas', () => {
    expect(toCsv([['a,b', 'say "hi"', 'two\nlines', '=SUM(A1)', '-1', 3.5, -2, null, undefined]])).toBe(
      '"a,b","say ""hi""","two\nlines",\'=SUM(A1),\'-1,3.5,-2,,\r\n'
    )
  })

  it('writes the monitors, a blank line and the incidents', () => {
    expect(slaPeriodCsv(period).split('\r\n')).toEqual([
      'monitor_id,monitor,availability_percent,availability_outcome,budget_used_percent,latency_ms,latency_outcome',
      '11,"Shop, ""main""",99.9512,met,48.8,212.3,',
      "12,'=cmd,,no_data,,,",
      '',
      'incident_id,incident,impact,started_at,resolved_at,response_outcome,response_minutes,response_deadline,resolution_outcome,resolution_minutes,resolution_deadline',
      '5,"Checkout down\nagain",major_outage,2026-10-03T08:00:00.000Z,2026-10-03T09:30:00.000Z,met,12.5,,missed,90,2026-10-03T09:00:00.000Z',
      '',
    ])
  })

  it('names the file after the sla and period', () => {
    expect(csvFileName('Acme Hosting / Österreich', '2026-10-01')).toBe('acme-hosting-österreich-2026-10-01.csv')
    expect(csvFileName('!!!', '2026-10-01')).toBe('sla-2026-10-01.csv')
  })
})
