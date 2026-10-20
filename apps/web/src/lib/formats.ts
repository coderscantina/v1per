import type { DateFormat, DisplayZone, NumberFormat, TimeFormat, WeekStart } from '~/generated'

/** How the reader wants numbers, dates and times written; absent parts follow `language`. */
export interface Formats {
  language: string
  number?: NumberFormat
  date?: DateFormat
  time?: TimeFormat
  zone?: DisplayZone
  week?: WeekStart
}

/** How numbers are written: a language's own way, or a chosen grouping and decimal mark. */
export type NumberLocale = string | { group: string; decimal: string }

const marks: Record<NumberFormat, { group: string; decimal: string }> = {
  comma: { group: ',', decimal: '.' },
  period: { group: '.', decimal: ',' },
  space: { group: '\u202f', decimal: ',' },
  space_period: { group: '\u202f', decimal: '.' },
  apostrophe: { group: '’', decimal: '.' },
}

export const numberLocale = (formats: Formats): NumberLocale => (formats.number ? marks[formats.number] : formats.language)

const numberCache = new Map<string, Intl.NumberFormat>()

/**
 * `value` written for `locale`. Chosen marks replace those of US English, so no language has to
 * write numbers exactly that way, as none writes "1 234.5".
 */
export function formatNumber(locale: NumberLocale, value: number, options: Intl.NumberFormatOptions = {}): string {
  const tag = typeof locale === 'string' ? locale : 'en-US'
  const key = `${tag}:${JSON.stringify(options)}`
  let formatter = numberCache.get(key)
  if (!formatter) {
    formatter = new Intl.NumberFormat(tag, options)
    numberCache.set(key, formatter)
  }
  if (typeof locale === 'string') return formatter.format(value)
  return formatter
    .formatToParts(value)
    .map((part) => (part.type === 'group' ? locale.group : part.type === 'decimal' ? locale.decimal : part.value))
    .join('')
}

const weekDays: Record<WeekStart, number> = { sunday: 0, monday: 1, saturday: 6 }

/** The first day of the week as 0 (Sunday) to 6: the choice, or the language's, or Monday. */
export function weekStart(formats: Formats): number {
  if (formats.week) return weekDays[formats.week]
  const info = new Intl.Locale(formats.language) as Intl.Locale & {
    getWeekInfo?: () => { firstDay: number }
    weekInfo?: { firstDay: number }
  }
  const firstDay = (info.getWeekInfo?.() ?? info.weekInfo)?.firstDay ?? 1
  return firstDay % 7
}

const hasTime = (options: Intl.DateTimeFormatOptions) =>
  options.timeStyle !== undefined || options.hour !== undefined || options.minute !== undefined

/** `options` with the reader's clock and zone; a zone the caller set, such as a schedule's, wins. */
export function withFormats(formats: Formats, options: Intl.DateTimeFormatOptions): Intl.DateTimeFormatOptions {
  const next = { ...options }
  if (formats.time && hasTime(options)) next.hourCycle = formats.time === 'h12' ? 'h12' : 'h23'
  if (formats.zone === 'utc' && !options.timeZone) next.timeZone = 'UTC'
  return next
}

const separators: Record<DateFormat, string> = { day_month_year: '.', month_day_year: '/', year_month_day: '-' }
const orders: Record<DateFormat, ReadonlyArray<'day' | 'month' | 'year'>> = {
  day_month_year: ['day', 'month', 'year'],
  month_day_year: ['month', 'day', 'year'],
  year_month_day: ['year', 'month', 'day'],
}

/**
 * A date in the reader's digit order, "20.10.2026", or the language's medium date, "Oct 20,
 * 2026", without a choice. `timeZone` overrides the reader's zone, as for a UTC day.
 */
export function formatDate(formats: Formats, ms: number, timeZone?: string): string {
  const zone = withFormats(formats, { timeZone }).timeZone
  if (!formats.date) return new Intl.DateTimeFormat(formats.language, { dateStyle: 'medium', timeZone: zone }).format(ms)
  const parts = new Intl.DateTimeFormat('en-US', { year: 'numeric', month: '2-digit', day: '2-digit', timeZone: zone }).formatToParts(ms)
  const part = (type: string) => parts.find((entry) => entry.type === type)?.value ?? ''
  return orders[formats.date].map(part).join(separators[formats.date])
}

/** A time of day, "14:30" or "2:30 PM", in the reader's clock and zone. */
export const formatTime = (formats: Formats, ms: number, timeZone?: string) =>
  new Intl.DateTimeFormat(formats.language, withFormats(formats, { timeStyle: 'short', timeZone })).format(ms)

/** A date and time; in UTC it says so, as other readers of the same page may not be. */
export function formatDateTime(formats: Formats, ms: number, timeZone?: string): string {
  const text = `${formatDate(formats, ms, timeZone)}, ${formatTime(formats, ms, timeZone)}`
  return formats.zone === 'utc' && !timeZone ? `${text} UTC` : text
}
