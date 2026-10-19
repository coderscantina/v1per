import type { Locale } from '~/generated/Locale'

import { t } from './messages'

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

/** Milliseconds as "120 ms" or "1.2 s". */
export function formatLatency(ms: number, locale: Locale): string {
  if (ms < 1000) return `${new Intl.NumberFormat(locale, { maximumFractionDigits: 0 }).format(ms)} ms`
  return `${new Intl.NumberFormat(locale, { maximumFractionDigits: 1 }).format(ms / 1000)} s`
}
