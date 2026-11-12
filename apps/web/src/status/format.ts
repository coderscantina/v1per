import type { Locale } from '~/generated/Locale'

import { t } from './messages'
import type { BarTime } from './view'

/**
 * An uptime ratio as a percentage with up to two decimals, rounded down so a day with one
 * failed check never reads 100 %.
 */
export function formatUptime(ratio: number, locale: Locale): string {
  const floored = Math.floor(ratio * 10_000) / 10_000
  return new Intl.NumberFormat(locale, { style: 'percent', maximumFractionDigits: 2 }).format(floored)
}

/** A bar's `YYYY-MM-DD` as a date. The day is already local to the page, so it is read as UTC. */
export function formatDay(day: string, locale: Locale): string {
  return new Intl.DateTimeFormat(locale, { dateStyle: 'medium', timeZone: 'UTC' }).format(new Date(`${day}T00:00:00Z`))
}

const HOUR_MS = 3_600_000

const hourFormat = (locale: Locale) => new Intl.DateTimeFormat(locale, { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' })

/** Where a bar starts: its day, or the start of its hour in the visitor's timezone. */
export function formatBarStart(time: BarTime, locale: Locale): string {
  return 'day' in time ? formatDay(time.day, locale) : hourFormat(locale).format(time.hour)
}

/** Where an hour's bar ends, in the visitor's timezone. */
export function formatBarEnd(time: { hour: number }, locale: Locale): string {
  return hourFormat(locale).format(time.hour + HOUR_MS)
}

/** A bar's whole span: its day, or its hour as "Nov 12, 14:00 – 15:00". */
export function formatBar(time: BarTime, locale: Locale): string {
  return 'day' in time ? formatDay(time.day, locale) : hourFormat(locale).formatRange(time.hour, time.hour + HOUR_MS)
}

/** A moment in the visitor's own timezone, named. */
export function formatTime(at: number, locale: Locale): string {
  return new Intl.DateTimeFormat(locale, {
    day: 'numeric',
    month: 'short',
    year: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
    timeZoneName: 'short',
  }).format(at)
}

/** A day in the visitor's own timezone, as "Oct 19, 2026". */
export function formatDate(at: number, locale: Locale): string {
  return new Intl.DateTimeFormat(locale, { dateStyle: 'medium' }).format(at)
}

/** A day in the visitor's own timezone without the year, as "Oct 25". */
export function formatShortDate(at: number, locale: Locale): string {
  return new Intl.DateTimeFormat(locale, { month: 'short', day: 'numeric' }).format(at)
}

/** A month heading, as "October 2026". */
export function formatMonth(at: number, locale: Locale): string {
  return new Intl.DateTimeFormat(locale, { month: 'long', year: 'numeric' }).format(at)
}

const ages: [Intl.RelativeTimeFormatUnit, number][] = [
  ['year', 365 * 86_400_000],
  ['month', 30 * 86_400_000],
  ['week', 7 * 86_400_000],
  ['day', 86_400_000],
  ['hour', 3_600_000],
  ['minute', 60_000],
]

/** How long ago `at` was, in the largest whole unit, as "3 weeks ago" or "yesterday". */
export function formatAge(at: number, now: number, locale: Locale): string {
  const elapsed = Math.max(0, now - at)
  const format = new Intl.RelativeTimeFormat(locale, { numeric: 'auto' })
  for (const [unit, size] of ages) if (elapsed >= size) return format.format(-Math.floor(elapsed / size), unit)
  return format.format(0, 'second')
}

/** Whether two moments fall on the same day in the visitor's timezone. */
export function sameDay(a: number, b: number): boolean {
  return new Date(a).toDateString() === new Date(b).toDateString()
}

/** Only the clock time, for "Updated 14:05". */
export function formatClock(at: number, locale: Locale): string {
  return new Intl.DateTimeFormat(locale, { hour: '2-digit', minute: '2-digit' }).format(at)
}

/** A duration in minutes, hours or days, coarse enough to read at a glance. */
export function formatDuration(ms: number): string {
  const minutes = Math.max(1, Math.round(ms / 60_000))
  if (minutes < 60) return t('duration.minutes', { n: minutes })
  const hours = Math.floor(minutes / 60)
  if (hours < 24) return t('duration.hours', { h: hours, m: minutes % 60 })
  return t('duration.days', { d: Math.floor(hours / 24), h: hours % 24 })
}

/** How long an incident lasted, as "Resolved after 2 h 5 min", or "Ongoing for 12 min" while open. */
export function incidentDuration(startedAt: number, resolvedAt: number | undefined, now: number): string {
  return resolvedAt === undefined
    ? t('incidents.ongoingFor', { duration: formatDuration(now - startedAt) })
    : t('incidents.resolvedAfter', { duration: formatDuration(resolvedAt - startedAt) })
}

/** Milliseconds as "120 ms" or "1.2 s". */
export function formatLatency(ms: number, locale: Locale): string {
  if (ms < 1000) return `${new Intl.NumberFormat(locale, { maximumFractionDigits: 0 }).format(ms)} ms`
  return `${new Intl.NumberFormat(locale, { maximumFractionDigits: 1 }).format(ms / 1000)} s`
}
