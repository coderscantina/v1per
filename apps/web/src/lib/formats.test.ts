import { describe, expect, it } from 'vitest'

import { formatDate, formatDateTime, formatNumber, numberLocale, weekStart, withFormats } from './formats'

const at = Date.UTC(2026, 9, 20, 14, 30)

describe('formats', () => {
  it('writes dates in the chosen digit order', () => {
    expect(formatDate({ language: 'en', date: 'day_month_year', zone: 'utc' }, at)).toBe('20.10.2026')
    expect(formatDate({ language: 'en', date: 'month_day_year', zone: 'utc' }, at)).toBe('10/20/2026')
    expect(formatDate({ language: 'en', date: 'year_month_day', zone: 'utc' }, at)).toBe('2026-10-20')
  })

  it('applies the clock and the zone unless the caller names a zone', () => {
    expect(formatDateTime({ language: 'en', date: 'year_month_day', time: 'h24', zone: 'utc' }, at)).toBe('2026-10-20, 14:30 UTC')
    expect(withFormats({ language: 'en', zone: 'utc' }, { timeZone: 'Europe/Vienna' }).timeZone).toBe('Europe/Vienna')
    expect(withFormats({ language: 'en', time: 'h12' }, { dateStyle: 'medium' }).hourCycle).toBeUndefined()
  })

  it('takes the number locale from the choice or the language', () => {
    expect(formatNumber(numberLocale({ language: 'en', number: 'period' }), 1234.5)).toBe('1.234,5')
    expect(formatNumber(numberLocale({ language: 'de', number: 'space_period' }), 1234.5)).toBe('1\u202f234.5')
    expect(formatNumber(numberLocale({ language: 'de' }), 1234.5)).toBe('1.234,5')
  })

  it('starts the week on the choice, else as the language does', () => {
    expect(weekStart({ language: 'en-US', week: 'saturday' })).toBe(6)
    expect(weekStart({ language: 'de' })).toBe(1)
  })
})
