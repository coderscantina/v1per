// From holocron (84fc54ec): apps/web/src/composables/useDateFormat.ts.
import { formatDate, formatDateTime, formatTime, withFormats } from '~/lib/formats'
import { useI18n } from '~/plugins/i18n'

import { useFormats } from './usePreferences'

const cache = new Map<string, Intl.DateTimeFormat>()

/**
 * Dates and times in the reader's formats: their digit order, clock and zone from their
 * preferences, words such as month names from the app language. `timeZone` arguments override
 * the reader's zone, for a UTC day or a schedule's own zone.
 */
export function useDateFormat() {
  const { locale } = useI18n()
  const { formats } = useFormats()
  /** A formatter for custom parts, such as an axis label, with the reader's clock and zone. */
  const format = (options: Intl.DateTimeFormatOptions) => {
    const applied = withFormats(formats.value, options)
    const key = `${locale.value}:${JSON.stringify(applied)}`
    let formatter = cache.get(key)
    if (!formatter) {
      formatter = new Intl.DateTimeFormat(locale.value, applied)
      cache.set(key, formatter)
    }
    return formatter
  }
  return {
    format,
    /** Unix milliseconds as a date, "Oct 20, 2026" or "20.10.2026". */
    date: (ms: number, timeZone?: string) => formatDate(formats.value, ms, timeZone),
    /** Unix milliseconds as a time of day. */
    time: (ms: number, timeZone?: string) => formatTime(formats.value, ms, timeZone),
    /** Unix milliseconds with the time, for tooltips and tables. */
    dateTime: (ms: number, timeZone?: string) => formatDateTime(formats.value, ms, timeZone),
    /**
     * How long before `now` a moment was, "25 seconds ago", in the largest whole unit; `narrow`
     * gives "25s ago" for tight spots.
     */
    relative: (ms: number, now: number, style: Intl.RelativeTimeFormatStyle = 'long') => {
      const seconds = Math.round((ms - now) / 1000)
      const formatter = new Intl.RelativeTimeFormat(locale.value, { numeric: 'auto', style })
      const steps = [
        [60, 'second'],
        [3600, 'minute'],
        [86_400, 'hour'],
      ] as const
      let divisor = 1
      for (const [limit, unit] of steps) {
        if (Math.abs(seconds) < limit) return formatter.format(Math.round(seconds / divisor), unit)
        divisor = limit
      }
      return formatter.format(Math.round(seconds / 86_400), 'day')
    },
  }
}
