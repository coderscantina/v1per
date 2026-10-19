// From holocron (84fc54ec): apps/web/src/composables/useDateFormat.ts.
import { useI18n } from '~/plugins/i18n'

const cache = new Map<string, Intl.DateTimeFormat>()

/** Dates of account records (keys, invitations) in the reader's locale and timezone. */
export function useDateFormat() {
  const { locale } = useI18n()
  const format = (options: Intl.DateTimeFormatOptions) => {
    const key = `${locale.value}:${JSON.stringify(options)}`
    let formatter = cache.get(key)
    if (!formatter) {
      formatter = new Intl.DateTimeFormat(locale.value, options)
      cache.set(key, formatter)
    }
    return formatter
  }
  return {
    /** Unix milliseconds as a date, "5 Oct 2026". */
    date: (ms: number) => format({ dateStyle: 'medium' }).format(ms),
    /** Unix milliseconds with the time, for tooltips. */
    dateTime: (ms: number) => format({ dateStyle: 'medium', timeStyle: 'short' }).format(ms),
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
