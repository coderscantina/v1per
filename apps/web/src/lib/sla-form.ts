import type {
  ChannelId,
  FieldErrorCode,
  MonitorId,
  Sla,
  SlaCombine,
  SlaInput,
  SlaPeriodKind,
  SlaTimeTarget,
  SlaTimeTargets,
} from '~/generated'
import type { MessageKey } from '~/plugins/i18n'

/**
 * The SLA editor's state. Numbers stay text as typed, so a half-typed value is reported rather
 * than lost. Business hours are kept per weekday (Sunday is 0, as in the protocol) and flattened
 * on save. Field errors use the server's argument paths, plus `hours.<weekday>` for one day.
 */

/** 1 to 100 characters after trimming, as the server checks. */
export const SLA_NAME_MAX_CHARS = 100
/** Upper bounds the server enforces. */
const LATENCY_MAX_MS = 600_000
const MINUTES_MAX = 525_600

export const periodKinds = ['month', 'quarter', 'year'] as const satisfies readonly SlaPeriodKind[]
export const combineModes = ['each', 'any_down'] as const satisfies readonly SlaCombine[]

/** The impacts a time target can be set for, worst first. */
export type SlaImpact = keyof SlaTimeTargets
export const slaImpacts = ['major_outage', 'partial_outage', 'degraded'] as const satisfies readonly SlaImpact[]

export const periodLabels: Record<SlaPeriodKind, MessageKey> = {
  month: 'slas.periods.month',
  quarter: 'slas.periods.quarter',
  year: 'slas.periods.year',
}

export const combineLabels: Record<SlaCombine, MessageKey> = {
  each: 'slas.combine.each',
  any_down: 'slas.combine.any_down',
}

export const combineHints: Record<SlaCombine, MessageKey> = {
  each: 'slas.combine.eachHint',
  any_down: 'slas.combine.any_downHint',
}

/** Sunday is 0. */
export const slaWeekdays = [0, 1, 2, 3, 4, 5, 6] as const

export interface HoursRange {
  /** `HH:MM`, as the time input writes it. */
  start: string
  /** `HH:MM`; `00:00` is the end of the day. */
  end: string
}

export interface TimeTargetForm {
  response: string
  resolution: string
}

export interface SlaForm {
  name: string
  timezone: string
  period: SlaPeriodKind
  /** `YYYY-MM-DD`. */
  startDate: string
  combine: SlaCombine
  /** Percent as typed, empty for no target. */
  availability: string
  latencyMs: string
  /** Minutes as typed, empty for no target. */
  times: Record<SlaImpact, TimeTargetForm>
  /** Seven lists, index 0 is Sunday. All empty: around the clock. */
  hours: HoursRange[][]
  /** `YYYY-MM-DD`, sorted. */
  holidays: string[]
  monitors: MonitorId[]
  channels: ChannelId[]
}

const emptyTimes = (): Record<SlaImpact, TimeTargetForm> => ({
  major_outage: { response: '', resolution: '' },
  partial_outage: { response: '', resolution: '' },
  degraded: { response: '', resolution: '' },
})

/** The first day of the current month, the usual contract start. */
const monthStart = (today: Date) =>
  `${today.getFullYear()}-${String(today.getMonth() + 1).padStart(2, '0')}-01`

export function emptySlaForm(timezone: string, today = new Date()): SlaForm {
  return {
    name: '',
    timezone,
    period: 'month',
    startDate: monthStart(today),
    combine: 'each',
    availability: '',
    latencyMs: '',
    times: emptyTimes(),
    hours: slaWeekdays.map(() => []),
    holidays: [],
    monitors: [],
    channels: [],
  }
}

const text = (value: number | undefined) => (value === undefined ? '' : String(value))

/** A stored end of `24:00` shows as `00:00`, which a time input can hold. */
const endOfDayIn = (end: string) => (end === '24:00' ? '00:00' : end)
const endOfDayOut = (end: string) => (end === '00:00' ? '24:00' : end)

export function formFromSla(sla: Sla): SlaForm {
  const times = emptyTimes()
  for (const impact of slaImpacts) {
    const target = sla.times?.[impact]
    times[impact] = { response: text(target?.response_minutes), resolution: text(target?.resolution_minutes) }
  }
  const hours: HoursRange[][] = slaWeekdays.map(() => [])
  for (const range of sla.hours ?? []) {
    hours[range.weekday]?.push({ start: range.start, end: endOfDayIn(range.end) })
  }
  for (const day of hours) day.sort((a, b) => a.start.localeCompare(b.start))
  return {
    name: sla.name,
    timezone: sla.timezone,
    period: sla.period,
    startDate: sla.start_date,
    combine: sla.combine,
    availability: text(sla.availability),
    latencyMs: text(sla.latency_ms),
    times,
    hours,
    holidays: [...(sla.holidays ?? [])].sort(),
    monitors: sla.monitors,
    channels: sla.channels ?? [],
  }
}

const optionalNumber = (value: string) => (value.trim() === '' ? undefined : Number(value.trim()))

/** What `slas.create` and `slas.update` send. */
export function toSlaInput(form: SlaForm): SlaInput {
  const times: SlaTimeTargets = {}
  for (const impact of slaImpacts) {
    const target: SlaTimeTarget = {
      response_minutes: optionalNumber(form.times[impact].response),
      resolution_minutes: optionalNumber(form.times[impact].resolution),
    }
    if (target.response_minutes !== undefined || target.resolution_minutes !== undefined) times[impact] = target
  }
  return {
    name: form.name.trim(),
    timezone: form.timezone.trim(),
    period: form.period,
    start_date: form.startDate,
    combine: form.combine,
    availability: optionalNumber(form.availability),
    latency_ms: optionalNumber(form.latencyMs),
    times,
    hours: form.hours.flatMap((ranges, weekday) =>
      ranges.map((range) => ({ weekday, start: range.start, end: endOfDayOut(range.end) }))
    ),
    holidays: form.holidays,
    monitors: form.monitors,
    channels: form.channels,
  }
}

/** Minutes of the day a range covers; an end of `00:00` is midnight at the end of the day. */
const minutes = (time: string) => {
  const match = /^(\d{2}):(\d{2})$/.exec(time)
  return match ? Number(match[1]) * 60 + Number(match[2]) : null
}

/** Whether one day's ranges each end after they start and none overlap. */
export function validDay(ranges: readonly HoursRange[]): boolean {
  const spans: [number, number][] = []
  for (const range of ranges) {
    const start = minutes(range.start)
    const end = range.end === '00:00' ? 1440 : minutes(range.end)
    if (start === null || end === null || end <= start) return false
    spans.push([start, end])
  }
  spans.sort((a, b) => a[0] - b[0])
  return spans.every((span, index) => index === 0 || span[0] >= (spans[index - 1]?.[1] ?? 0))
}

/** The next range to add to a day: office hours, or the hour after the last range. */
export function nextRange(ranges: readonly HoursRange[]): HoursRange {
  const last = ranges.at(-1)
  const after = last ? minutes(last.end) : null
  if (!last || after === null || after === 0) return { start: '09:00', end: '17:00' }
  const time = (value: number) => `${String(Math.floor(value / 60)).padStart(2, '0')}:${String(value % 60).padStart(2, '0')}`
  return { start: time(after), end: time(Math.min(after + 60, 1440) % 1440) }
}

/** Monday to Friday, 09:00 to 17:00. */
export const officeHours = (): HoursRange[][] =>
  slaWeekdays.map((day) => (day === 0 || day === 6 ? [] : [{ start: '09:00', end: '17:00' }]))

export type SlaFormErrors = Partial<Record<string, { code: FieldErrorCode; limit?: number }>>

const isWhole = (value: string) => /^\d+$/.test(value.trim())

/** What the browser can tell before sending; whether monitors and channels may be used is the server's. */
export function validateSla(form: SlaForm): SlaFormErrors {
  const errors: SlaFormErrors = {}
  const whole = (key: string, value: string, max: number) => {
    if (value.trim() === '') return
    const number = Number(value.trim())
    if (!isWhole(value) || number < 1 || number > max) errors[key] = { code: 'invalid_value', limit: max }
  }
  if (form.name.trim() === '') errors.name = { code: 'required' }
  if (form.timezone.trim() === '') errors.timezone = { code: 'required' }
  if (form.startDate === '') errors.start_date = { code: 'required' }
  else if (!/^\d{4}-\d{2}-\d{2}$/.test(form.startDate)) errors.start_date = { code: 'invalid_format' }
  if (form.availability.trim() !== '') {
    const value = form.availability.trim()
    const number = Number(value)
    if (!/^\d+(\.\d{1,3})?$/.test(value) || number <= 0 || number > 100) errors.availability = { code: 'invalid_value' }
  }
  whole('latency_ms', form.latencyMs, LATENCY_MAX_MS)
  for (const impact of slaImpacts) {
    whole(`times.${impact}.response_minutes`, form.times[impact].response, MINUTES_MAX)
    whole(`times.${impact}.resolution_minutes`, form.times[impact].resolution, MINUTES_MAX)
  }
  form.hours.forEach((ranges, weekday) => {
    if (!validDay(ranges)) errors[`hours.${weekday}`] = { code: 'invalid_value' }
  })
  if (form.monitors.length === 0) errors.monitors = { code: 'required' }
  return errors
}

/** Whole minutes as hours and minutes, such as `4 h` or `1 h 30 min`. */
export function durationParts(total: number): { hours: number; minutes: number } {
  return { hours: Math.floor(total / 60), minutes: total % 60 }
}
