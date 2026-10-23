import { describe, expect, it } from 'vitest'

import type { Sla } from '~/generated'

import { emptySlaForm, formFromSla, nextRange, officeHours, toSlaInput, validateSla, validDay } from './sla-form'

const sla: Sla = {
  id: '7',
  node: '3',
  name: 'Acme hosting',
  timezone: 'Europe/Vienna',
  period: 'quarter',
  start_date: '2026-01-01',
  combine: 'any_down',
  availability: 99.95,
  latency_ms: 300,
  times: { major_outage: { response_minutes: 15, resolution_minutes: 240 }, degraded: { resolution_minutes: 2880 } },
  hours: [
    { weekday: 1, start: '13:00', end: '24:00' },
    { weekday: 1, start: '08:00', end: '12:00' },
  ],
  holidays: ['2026-12-25', '2026-01-06'],
  monitors: ['11', '12'],
  channels: ['5'],
  revision: 4,
  created_at: 0,
  updated_at: 0,
}

describe('sla form', () => {
  it('round-trips stored terms, with midnight as the end of the day', () => {
    const form = formFromSla(sla)
    expect(form.hours[1]).toEqual([
      { start: '08:00', end: '12:00' },
      { start: '13:00', end: '00:00' },
    ])
    expect(form.times.partial_outage).toEqual({ response: '', resolution: '' })
    expect(toSlaInput(form)).toEqual({
      name: 'Acme hosting',
      timezone: 'Europe/Vienna',
      period: 'quarter',
      start_date: '2026-01-01',
      combine: 'any_down',
      availability: 99.95,
      latency_ms: 300,
      times: {
        major_outage: { response_minutes: 15, resolution_minutes: 240 },
        degraded: { response_minutes: undefined, resolution_minutes: 2880 },
      },
      hours: [
        { weekday: 1, start: '08:00', end: '12:00' },
        { weekday: 1, start: '13:00', end: '24:00' },
      ],
      holidays: ['2026-01-06', '2026-12-25'],
      monitors: ['11', '12'],
      channels: ['5'],
    })
  })

  it('leaves out targets that are not set', () => {
    const input = toSlaInput({ ...emptySlaForm('UTC'), name: 'Basic', monitors: ['1'] })
    expect(input.availability).toBeUndefined()
    expect(input.latency_ms).toBeUndefined()
    expect(input.times).toEqual({})
    expect(input.hours).toEqual([])
  })

  it('starts a new SLA on the first of the month', () => {
    expect(emptySlaForm('UTC', new Date(2026, 9, 23)).startDate).toBe('2026-10-01')
  })

  it('reports what the server would refuse', () => {
    const form = emptySlaForm('')
    Object.assign(form, { availability: '99.9999', latencyMs: '0', startDate: '' })
    form.times.major_outage.response = '1.5'
    form.hours[2] = [{ start: '17:00', end: '09:00' }]
    expect(Object.keys(validateSla(form)).sort()).toEqual([
      'availability',
      'hours.2',
      'latency_ms',
      'monitors',
      'name',
      'start_date',
      'times.major_outage.response_minutes',
      'timezone',
    ])
    Object.assign(form, { name: 'Ok', timezone: 'UTC', startDate: '2026-10-01', availability: '100', latencyMs: '', monitors: ['1'] })
    form.times.major_outage.response = '30'
    form.hours = officeHours()
    expect(validateSla(form)).toEqual({})
  })

  it('refuses overlapping ranges on a day and accepts touching ones', () => {
    expect(validDay([{ start: '08:00', end: '12:00' }, { start: '12:00', end: '00:00' }])).toBe(true)
    expect(validDay([{ start: '08:00', end: '12:00' }, { start: '11:00', end: '13:00' }])).toBe(false)
    expect(validDay([{ start: '00:00', end: '00:00' }])).toBe(true)
  })

  it('adds the next range after the last one', () => {
    expect(nextRange([])).toEqual({ start: '09:00', end: '17:00' })
    expect(nextRange([{ start: '08:00', end: '12:00' }])).toEqual({ start: '12:00', end: '13:00' })
    expect(nextRange([{ start: '18:00', end: '23:30' }])).toEqual({ start: '23:30', end: '00:00' })
  })
})
