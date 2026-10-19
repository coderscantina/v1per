import { describe, expect, it } from 'vitest'

import type { Page } from '~/generated'

import { accessFromPage, accessModes, parseList, slugify, toAccessArgs, toUpdateArgs, validateAccess, settingsFromPage, validateSettings } from './pages'

const page: Page = {
  id: 'p1',
  node: 'n1',
  access: { public: false, password: { set: true }, emails: ['ops@acme.test'], domains: ['acme.test'] },
  hostnames: [],
  path: '/s/acme',
  revision: 4,
  created_at: 0,
  updated_at: 0,
  slug: 'acme',
  title: 'Acme',
  default_locale: 'de',
  locales: ['de', 'en'],
  grace_minutes: 10,
}

describe('access form', () => {
  it('keeps a stored password unless it is replaced or cleared', () => {
    const form = accessFromPage(page.access)
    expect(form.emails).toBe('ops@acme.test')
    expect(toAccessArgs(form, page, 4)).toEqual({
      page: 'p1',
      expected_revision: 4,
      public: false,
      password: undefined,
      emails: ['ops@acme.test'],
      domains: ['acme.test'],
    })
    expect(toAccessArgs({ ...form, passwordAction: 'set', password: 'hunter2' }, page, 4).password).toEqual({ value: 'hunter2', set: true })
    expect(toAccessArgs({ ...form, passwordAction: 'clear' }, page, 4).password).toEqual({ clear: true, set: false })
  })

  it('reads lists split by lines, commas and spaces, lowercased and without repeats', () => {
    expect(parseList('Ops@Acme.test, a@b.test\n\nops@acme.test  c@d.test')).toEqual(['ops@acme.test', 'a@b.test', 'c@d.test'])
    expect(toAccessArgs({ ...accessFromPage(page.access), domains: '@Acme.test example.org' }, page, 4).domains).toEqual(['acme.test', 'example.org'])
  })

  it('flags a missing new password and malformed addresses or domains', () => {
    const form = accessFromPage(page.access)
    expect(validateAccess(form)).toEqual({})
    expect(Object.keys(validateAccess({ ...form, passwordAction: 'set' }))).toEqual(['password'])
    expect(Object.keys(validateAccess({ ...form, emails: 'not-an-address', domains: 'acme' }))).toEqual(['emails', 'domains'])
  })

  it('names the ways in: public, the restricted modes, or team only', () => {
    expect(accessModes({ public: true, password: { set: true } }, 2)).toEqual(['public'])
    expect(accessModes(page.access, 1)).toEqual(['password', 'email', 'tokens'])
    expect(accessModes({ public: false, password: { set: false }, emails: [], domains: [] }, 0)).toEqual(['team'])
  })
})

describe('settings form', () => {
  it('suggests a slug from the title', () => {
    expect(slugify('  Acme Cloud — Störungen! ')).toBe('acme-cloud-storungen')
  })

  it('sends every field with the default locale first and keeps grace while drafting', () => {
    const form = { ...settingsFromPage(page), locales: ['en' as const], autoIncidents: 'draft' as const, graceMinutes: '99' }
    expect(toUpdateArgs(form, page, 4)).toMatchObject({ expected_revision: 4, default_locale: 'de', locales: ['de', 'en'], grace_minutes: 10 })
    expect(Object.keys(validateSettings({ ...form, title: ' ', slug: 'Bad slug', autoIncidents: 'auto_publish', graceMinutes: '2000' }))).toEqual([
      'slug',
      'title',
      'grace_minutes',
    ])
  })
})
