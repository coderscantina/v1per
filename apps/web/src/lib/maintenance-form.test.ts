import { describe, expect, it } from 'vitest'

import type { Maintenance } from '~/generated'

import {
  emptyMaintenanceForm,
  formFromMaintenance,
  previewSchedule,
  strategies,
  toSchedule,
  validateMaintenance,
} from './maintenance-form'

describe('maintenance strategy forms', () => {
  it('sends each strategy with only its own fields', () => {
    const form = emptyMaintenanceForm()
    Object.assign(form, {
      title: 'Patch day',
      startDate: '2026-11-01T02:00',
      endDate: '2026-11-01T04:00',
      weekdays: [6, 0],
      daysOfMonth: [15, 1],
      timezone: 'Europe/Vienna',
    })
    const schedules = Object.fromEntries(strategies.map((strategy) => [strategy, toSchedule({ ...form, strategy })]))
    expect(schedules).toEqual({
      manual: { strategy: 'manual', timezone: 'Europe/Vienna' },
      single: { strategy: 'single', start_date: '2026-11-01T02:00', end_date: '2026-11-01T04:00', timezone: 'Europe/Vienna' },
      'recurring-interval': {
        strategy: 'recurring-interval',
        interval_day: 1,
        start_time: '02:00',
        end_time: '03:00',
        start_date: '2026-11-01T02:00',
        end_date: '2026-11-01T04:00',
        timezone: 'Europe/Vienna',
      },
      'recurring-weekday': expect.objectContaining({ strategy: 'recurring-weekday', weekdays: [0, 6], start_time: '02:00' }),
      'recurring-day-of-month': expect.objectContaining({ days_of_month: [1, 15], last_day_of_month: false }),
      cron: expect.objectContaining({ strategy: 'cron', cron: '30 3 * * *', duration: 3600 }),
    })
    expect(schedules.cron).not.toHaveProperty('start_time')
  })

  it('uses the instance timezone when none is picked', () => {
    expect(toSchedule(emptyMaintenanceForm('manual'))).toEqual({ strategy: 'manual', timezone: undefined })
  })

  it('asks for what each strategy needs', () => {
    const errors = (patch: Partial<ReturnType<typeof emptyMaintenanceForm>>) =>
      Object.keys(validateMaintenance({ ...emptyMaintenanceForm(), title: 'x', ...patch }))
    expect(errors({ strategy: 'manual', title: '' })).toEqual(['title'])
    expect(errors({ strategy: 'single' })).toEqual(['schedule.start_date', 'schedule.end_date'])
    expect(errors({ strategy: 'recurring-interval', intervalDay: '0' })).toEqual(['schedule.interval_day'])
    expect(errors({ strategy: 'recurring-weekday', startTime: '' })).toEqual(['schedule.start_time', 'schedule.weekdays'])
    expect(errors({ strategy: 'recurring-day-of-month', lastDayOfMonth: true })).toEqual([])
    expect(errors({ strategy: 'cron', cron: ' ', durationMinutes: 'x' })).toEqual(['schedule.cron', 'schedule.duration'])
  })

  it('previews only a complete schedule with windows', () => {
    expect(previewSchedule(emptyMaintenanceForm('manual'))).toBeNull()
    expect(previewSchedule(emptyMaintenanceForm('single'))).toBeNull()
    expect(previewSchedule({ ...emptyMaintenanceForm('recurring-weekday'), weekdays: [1] })).toMatchObject({ weekdays: [1] })
  })

  it('reads a stored cron maintenance back in minutes', () => {
    const stored: Maintenance = {
      id: '2',
      node: '1',
      title: 'Backups',
      description: '',
      schedule: { strategy: 'cron', cron: '0 1 * * 0', duration: 5400, timezone: 'UTC' },
      monitors: ['4'],
      active: true,
      status: 'scheduled',
      windows: [],
      revision: 1,
      created_at: 0,
      updated_at: 0,
    }
    const form = formFromMaintenance(stored)
    expect(form).toMatchObject({ strategy: 'cron', cron: '0 1 * * 0', durationMinutes: '90', timezone: 'UTC', monitors: ['4'] })
    expect(toSchedule(form)).toEqual(stored.schedule)
  })
})
