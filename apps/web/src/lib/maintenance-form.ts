import type {
  FieldErrorCode,
  Maintenance,
  MaintenanceSchedule,
  MaintenanceStatus,
  MaintenanceStrategy,
  MonitorId,
} from '~/generated'
import type { MessageKey } from '~/plugins/i18n'

/**
 * The maintenance editor's state, one shape for Kuma's six strategies: switching the strategy
 * keeps what was typed for the others, and only the chosen strategy's fields are sent. Dates
 * and times stay in the `datetime-local` and `time` inputs' own formats, which are the
 * schedule's (`YYYY-MM-DDTHH:MM`, `HH:MM`). Field errors use the server's argument paths.
 */

export const strategies = [
  'manual',
  'single',
  'recurring-interval',
  'recurring-weekday',
  'recurring-day-of-month',
  'cron',
] as const satisfies readonly MaintenanceStrategy[]

export const strategyLabels: Record<MaintenanceStrategy, MessageKey> = {
  manual: 'maintenance.strategies.manual',
  single: 'maintenance.strategies.single',
  'recurring-interval': 'maintenance.strategies.recurring-interval',
  'recurring-weekday': 'maintenance.strategies.recurring-weekday',
  'recurring-day-of-month': 'maintenance.strategies.recurring-day-of-month',
  cron: 'maintenance.strategies.cron',
}

export const statusLabels: Record<MaintenanceStatus, MessageKey> = {
  inactive: 'maintenance.status.inactive',
  scheduled: 'maintenance.status.scheduled',
  under_maintenance: 'maintenance.status.under_maintenance',
  ended: 'maintenance.status.ended',
  unknown: 'maintenance.status.unknown',
}

/** The dot colour. A word always goes with it. */
export const statusFill: Record<MaintenanceStatus, string> = {
  inactive: 'bg-status-none',
  scheduled: 'bg-status-pending',
  under_maintenance: 'bg-status-maintenance',
  ended: 'bg-status-none',
  unknown: 'bg-status-none',
}

/** Sunday is 0, as in Kuma and the schedule. */
export const weekdays = [1, 2, 3, 4, 5, 6, 0] as const
export const daysOfMonth = Array.from({ length: 31 }, (_, index) => index + 1)

/** Strategies that repeat at a time of day. */
const timed = ['recurring-interval', 'recurring-weekday', 'recurring-day-of-month'] as const
export const usesTimeOfDay = (strategy: MaintenanceStrategy) => timed.some((entry) => entry === strategy)
/** Strategies that may be limited to a date range. */
export const usesDateRange = (strategy: MaintenanceStrategy) => usesTimeOfDay(strategy) || strategy === 'cron'

export interface MaintenanceForm {
  title: string
  description: string
  strategy: MaintenanceStrategy
  /** `YYYY-MM-DDTHH:MM`; the window of `single`, else the range the others repeat in. */
  startDate: string
  endDate: string
  /** `HH:MM`. */
  startTime: string
  endTime: string
  weekdays: number[]
  daysOfMonth: number[]
  lastDayOfMonth: boolean
  intervalDay: string
  cron: string
  /** Minutes each cron window lasts, as in Kuma's editor; the schedule takes seconds. */
  durationMinutes: string
  /** An IANA zone; empty uses the instance timezone. */
  timezone: string
  monitors: MonitorId[]
}

export function emptyMaintenanceForm(strategy: MaintenanceStrategy = 'single'): MaintenanceForm {
  return {
    title: '',
    description: '',
    strategy,
    startDate: '',
    endDate: '',
    startTime: '02:00',
    endTime: '03:00',
    weekdays: [],
    daysOfMonth: [],
    lastDayOfMonth: false,
    intervalDay: '1',
    cron: '30 3 * * *',
    durationMinutes: '60',
    timezone: '',
    monitors: [],
  }
}

export function formFromMaintenance(maintenance: Maintenance): MaintenanceForm {
  const { schedule } = maintenance
  const base = emptyMaintenanceForm(schedule.strategy)
  return {
    ...base,
    title: maintenance.title,
    description: maintenance.description,
    startDate: schedule.start_date ?? '',
    endDate: schedule.end_date ?? '',
    startTime: schedule.start_time ?? base.startTime,
    endTime: schedule.end_time ?? base.endTime,
    weekdays: schedule.weekdays ?? [],
    daysOfMonth: schedule.days_of_month ?? [],
    lastDayOfMonth: schedule.last_day_of_month ?? false,
    intervalDay: schedule.interval_day === undefined ? base.intervalDay : String(schedule.interval_day),
    cron: schedule.cron ?? base.cron,
    durationMinutes: schedule.duration === undefined ? base.durationMinutes : String(Math.round(schedule.duration / 60)),
    timezone: schedule.timezone ?? '',
    monitors: maintenance.monitors,
  }
}

const optional = (value: string) => (value.trim() === '' ? undefined : value.trim())
/** Text as a number; `NaN` when it is not one, so validation and the server both refuse it. */
const numberOf = (value: string) => (value.trim() === '' ? Number.NaN : Number(value))

/** The chosen strategy's schedule, without the fields of the others. */
export function toSchedule(form: MaintenanceForm): MaintenanceSchedule {
  const timezone = optional(form.timezone)
  const range = { start_date: optional(form.startDate), end_date: optional(form.endDate) }
  const times = { start_time: form.startTime, end_time: form.endTime }
  switch (form.strategy) {
    case 'manual':
      return { strategy: 'manual', timezone }
    case 'single':
      return { strategy: 'single', ...range, timezone }
    case 'recurring-interval':
      return { strategy: 'recurring-interval', interval_day: numberOf(form.intervalDay), ...times, ...range, timezone }
    case 'recurring-weekday':
      return { strategy: 'recurring-weekday', weekdays: [...form.weekdays].sort((a, b) => a - b), ...times, ...range, timezone }
    case 'recurring-day-of-month':
      return {
        strategy: 'recurring-day-of-month',
        days_of_month: [...form.daysOfMonth].sort((a, b) => a - b),
        last_day_of_month: form.lastDayOfMonth,
        ...times,
        ...range,
        timezone,
      }
    case 'cron':
      return { strategy: 'cron', cron: form.cron.trim(), duration: numberOf(form.durationMinutes) * 60, ...range, timezone }
  }
}

export type MaintenanceFormErrors = Partial<Record<string, { code: FieldErrorCode }>>

const isWhole = (value: string) => /^\d+$/.test(value.trim()) && Number(value) > 0
const isTime = (value: string) => /^\d{2}:\d{2}$/.test(value)

/** What the browser can tell before sending; the schedule's own rules are the server's. */
export function validateSchedule(form: MaintenanceForm): MaintenanceFormErrors {
  const errors: MaintenanceFormErrors = {}
  const required = (key: string, missing: boolean) => {
    if (missing) errors[key] = { code: 'required' }
  }
  const whole = (key: string, value: string) => {
    if (value.trim() === '') errors[key] = { code: 'required' }
    else if (!isWhole(value)) errors[key] = { code: 'invalid_format' }
  }
  if (form.strategy === 'single') {
    required('schedule.start_date', form.startDate === '')
    required('schedule.end_date', form.endDate === '')
  }
  if (usesTimeOfDay(form.strategy)) {
    required('schedule.start_time', !isTime(form.startTime))
    required('schedule.end_time', !isTime(form.endTime))
  }
  switch (form.strategy) {
    case 'recurring-interval':
      whole('schedule.interval_day', form.intervalDay)
      break
    case 'recurring-weekday':
      required('schedule.weekdays', form.weekdays.length === 0)
      break
    case 'recurring-day-of-month':
      required('schedule.days_of_month', form.daysOfMonth.length === 0 && !form.lastDayOfMonth)
      break
    case 'cron':
      required('schedule.cron', form.cron.trim() === '')
      whole('schedule.duration', form.durationMinutes)
      break
  }
  return errors
}

export function validateMaintenance(form: MaintenanceForm): MaintenanceFormErrors {
  const errors = validateSchedule(form)
  if (form.title.trim() === '') errors.title = { code: 'required' }
  return errors
}

/** The schedule to preview, or `null` while it is incomplete; `manual` has no windows. */
export function previewSchedule(form: MaintenanceForm): MaintenanceSchedule | null {
  if (form.strategy === 'manual') return null
  return Object.keys(validateSchedule(form)).length === 0 ? toSchedule(form) : null
}
