import { describe, expect, it } from 'vitest'

import type { PageBar, PageSnapshot, PublicComponent } from '~/generated'

import { formatAge, formatUptime } from './format'
import { byMonth, pageView, worst } from './view'

const day = (index: number): PageBar => ({ day: `2026-07-${String(index + 1).padStart(2, '0')}` })

function component(id: string, name: string, extra: Partial<PublicComponent> = {}): PublicComponent {
  return { id, name, description: '', bars: [day(0), day(1)], ...extra }
}

function snapshot(extra: Partial<PageSnapshot> = {}): PageSnapshot {
  return {
    appearance: { title: 'Acme status', theme: 'auto', accent: 'blue', default_locale: 'en', locales: ['en'] },
    description: '',
    footer: '',
    timezone: 'Europe/Vienna',
    locale: 'en',
    audience: 'public',
    status: 'operational',
    groups: [],
    incidents: [],
    maintenance: [],
    cursor: 'page:1@0',
    generated_at: 0,
    ...extra,
  }
}

describe('pageView', () => {
  it('resolves bar incidents to titles and marks days without data', () => {
    const view = pageView(
      snapshot({
        groups: [
          {
            id: 'g1',
            name: 'Services',
            default_open: true,
            components: [
              component('c1', 'API', {
                status: 'major_outage',
                bars: [
                  { day: '2026-07-01' },
                  { day: '2026-07-02', status: 'partial_outage', uptime: 0.97, incidents: ['i1'], utc_approximate: true },
                ],
              }),
              component('c2', 'Email'),
            ],
          },
        ],
        incidents: [
          { id: 'i1', title: 'API is down', state: 'resolved', started_at: 0, resolved_at: 1, components: ['c1'], updates: [] },
        ],
      }),
      '/s/acme'
    )
    const [api, email] = view.groups[0]?.components ?? []
    expect(api?.bars).toEqual([
      { day: '2026-07-01', status: 'none', uptime: undefined, approximate: false, maintenance: false, incidents: [] },
      { day: '2026-07-02', status: 'partial_outage', uptime: 0.97, approximate: true, maintenance: false, incidents: ['API is down'] },
    ])
    expect(email?.status).toBe('none')
    expect(view.groups[0]?.status).toBe('major_outage')
  })

  it('keeps open incidents and takes each impact from the newest update naming it', () => {
    const view = pageView(
      snapshot({
        groups: [{ id: 'g', name: '', default_open: true, components: [component('c1', 'API'), component('c2', 'Email')] }],
        incidents: [
          {
            id: 'open',
            title: 'Payments are slow',
            state: 'identified',
            impact: 'partial_outage',
            started_at: 10,
            // Not on this page: dropped from the affected list.
            components: ['c1', 'c2', 'hidden'],
            updates: [
              { state: 'identified', message: 'Found it.', created_at: 20, impacts: [{ component: 'c2', impact: 'degraded' }] },
              {
                state: 'investigating',
                message: 'Looking.',
                created_at: 10,
                impacts: [
                  { component: 'c1', impact: 'partial_outage' },
                  { component: 'c2', impact: 'major_outage' },
                ],
              },
            ],
          },
          { id: 'old', title: 'Old', state: 'resolved', started_at: 1, resolved_at: 2, components: ['c1'], updates: [] },
        ],
      }),
      '/s/acme'
    )
    expect(view.active.map((incident) => incident.id)).toEqual(['open'])
    // Only what each update changed; the hidden component has no name and is left out.
    expect(view.active[0]?.timeline.map((entry) => [entry.message, entry.changes])).toEqual([
      ['Found it.', [{ name: 'Email', impact: 'degraded' }]],
      [
        'Looking.',
        [
          { name: 'API', impact: 'partial_outage' },
          { name: 'Email', impact: 'major_outage' },
        ],
      ],
    ])
    expect(view.active[0]?.affected).toEqual([
      { name: 'API', impact: 'partial_outage' },
      { name: 'Email', impact: 'degraded' },
    ])
  })

  it('puts maintenance under way first and names its components', () => {
    const view = pageView(
      snapshot({
        groups: [{ id: 'g', name: '', default_open: true, components: [component('c1', 'API')] }],
        maintenance: [
          { title: 'Later', description: '', active: false, starts_at: 500, ends_at: 600, components: ['c1'] },
          { title: 'Now', description: '', active: true, components: ['c1'] },
        ],
      }),
      ''
    )
    expect(view.maintenance.map((window) => [window.title, window.components])).toEqual([
      ['Now', ['API']],
      ['Later', ['API']],
    ])
  })

  it('sums up uptime, incidents and the next maintenance', () => {
    const view = pageView(
      snapshot({
        groups: [
          {
            id: 'g',
            name: '',
            default_open: true,
            components: [component('c1', 'API', { uptime_90d: 0.99 }), component('c2', 'Email', { uptime_90d: 0.97 }), component('c3', 'New')],
          },
        ],
        incidents: [{ id: 'i1', title: 'Slow', state: 'resolved', started_at: 0, resolved_at: 1, components: [], updates: [] }],
        maintenance: [
          { title: 'Later', description: '', active: false, starts_at: 900, components: [] },
          { title: 'Sooner', description: '', active: false, starts_at: 500, components: [] },
        ],
      }),
      ''
    )
    expect(view.figures.uptime).toBeCloseTo(0.98)
    expect(view.figures.incidents).toBe(1)
    expect(view.figures.maintenance).toBe(500)
    expect(pageView(snapshot(), '').figures).toEqual({ uptime: undefined, incidents: 0, maintenance: undefined })
  })

  it('places the logo below the base', () => {
    const appearance = { ...snapshot().appearance, logo: '/logo?v=5' }
    expect(pageView(snapshot({ appearance }), '/s/acme').logo).toBe('/s/acme/logo?v=5')
    expect(pageView(snapshot(), '').logo).toBeUndefined()
  })
})

describe('timeline changes', () => {
  it('leaves out an impact an update repeats', () => {
    const view = pageView(
      snapshot({
        groups: [{ id: 'g', name: '', default_open: true, components: [component('c1', 'API')] }],
        incidents: [
          {
            id: 'i',
            title: 'Slow',
            state: 'monitoring',
            started_at: 0,
            components: ['c1'],
            updates: [
              { state: 'monitoring', message: '', created_at: 2, impacts: [{ component: 'c1', impact: 'degraded' }] },
              { state: 'investigating', message: '', created_at: 1, impacts: [{ component: 'c1', impact: 'degraded' }] },
            ],
          },
        ],
      }),
      ''
    )
    expect(view.active[0]?.timeline.map((entry) => entry.changes.length)).toEqual([0, 1])
  })
})

describe('byMonth', () => {
  it('groups by start month and merges a month that comes back', () => {
    const at = (iso: string) => ({ startedAt: Date.parse(iso) })
    const groups = byMonth([at('2026-10-15T12:00:00'), at('2026-09-20T12:00:00'), at('2026-10-02T12:00:00')])
    expect(groups.map((group) => [group.key, group.items.length])).toEqual([
      ['2026-10', 2],
      ['2026-9', 1],
    ])
  })
})

describe('formatAge', () => {
  it('names the largest whole unit', () => {
    const now = Date.parse('2026-10-19T12:00:00Z')
    expect(formatAge(now - 30_000, now, 'en')).toBe('now')
    expect(formatAge(now - 5 * 60_000, now, 'en')).toBe('5 minutes ago')
    expect(formatAge(now - 3 * 86_400_000, now, 'en')).toBe('3 days ago')
    expect(formatAge(now - 40 * 86_400_000, now, 'en')).toBe('last month')
  })
})

describe('worst', () => {
  it('ranks maintenance above operational and below every outage', () => {
    expect(worst(['operational', 'maintenance'])).toBe('maintenance')
    expect(worst(['maintenance', 'degraded'])).toBe('degraded')
    expect(worst([])).toBe('none')
  })
})

describe('formatUptime', () => {
  it('rounds down so a failed check never reads 100 %', () => {
    expect(formatUptime(0.99999, 'en')).toBe('99.99%')
    expect(formatUptime(1, 'en')).toBe('100%')
  })
})
