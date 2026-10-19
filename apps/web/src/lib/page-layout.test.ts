import { describe, expect, it, vi } from 'vitest'
import { nextTick, shallowRef } from 'vue'

import { ApiError } from '~/api'
import type { PageDetail, PageGroup } from '~/generated'

import {
  addComponents,
  addGroup,
  canStep,
  draftFromGroups,
  moveComponent,
  moveGroup,
  removeComponent,
  removeGroup,
  stepComponent,
  toGroupInputs,
  useLayoutEditor,
  type LayoutDraft,
} from './page-layout'

const monitor = (id: string) => ({ type: 'monitor' as const, monitor: id })
const component = (id: string) => ({ type: 'component' as const, component: id })

const groups: PageGroup[] = [
  {
    id: 'g1',
    name: 'Web',
    default_open: true,
    components: [
      { id: 'c1', target: monitor('1'), name: 'API', internal: false },
      { id: 'c2', target: monitor('2'), name: 'Shop', display_name: 'Shop', internal: false },
    ],
  },
  { id: 'g2', name: 'Support', default_open: false, components: [{ id: 'c3', target: component('9'), name: 'Phone', internal: true }] },
]

const names = (draft: LayoutDraft) => draft.map((group) => group.components.map((entry) => entry.name))

const detail = (revision: number, layout: PageGroup[] = groups): PageDetail => ({
  page: { id: 'p1', node: 'n1', access: { public: false }, hostnames: [], path: '/s/acme', revision, created_at: 0, updated_at: 0, slug: 'acme', title: 'Acme' },
  groups: layout,
  share_tokens: [],
  smtp_configured: false,
})

const stale = () =>
  new ApiError(409, { error_code: 'stale_revision', message: '', current: 8 }, new Response(null, { status: 409 }))

describe('layout draft', () => {
  it('round-trips groups and sends overrides only when set', () => {
    expect(toGroupInputs(draftFromGroups(groups))).toEqual([
      {
        id: 'g1',
        name: 'Web',
        default_open: true,
        components: [
          { target: monitor('1'), display_name: undefined, internal: false },
          { target: monitor('2'), display_name: 'Shop', internal: false },
        ],
      },
      { id: 'g2', name: 'Support', default_open: false, components: [{ target: component('9'), display_name: undefined, internal: true }] },
    ])
  })

  it('reorders groups and components, within and across groups', () => {
    const draft = draftFromGroups(groups)
    expect(moveGroup(draft, 1, 0).map((group) => group.name)).toEqual(['Support', 'Web'])
    expect(names(moveComponent(draft, { group: 0, index: 0 }, { group: 0, index: 1 }))).toEqual([['Shop', 'API'], ['Phone']])
    expect(names(moveComponent(draft, { group: 0, index: 1 }, { group: 1, index: 0 }))).toEqual([['API'], ['Shop', 'Phone']])
    expect(names(draft)).toEqual([['API', 'Shop'], ['Phone']])
  })

  it('steps across group edges with the keyboard buttons and stops at the page edges', () => {
    const draft = draftFromGroups(groups)
    expect(names(stepComponent(draft, { group: 0, index: 1 }, 1))).toEqual([['API'], ['Shop', 'Phone']])
    expect(names(stepComponent(draft, { group: 1, index: 0 }, -1))).toEqual([['API', 'Shop', 'Phone'], []])
    expect(canStep(draft, { group: 0, index: 0 }, -1)).toBe(false)
    expect(canStep(draft, { group: 1, index: 0 }, 1)).toBe(false)
    expect(stepComponent(draft, { group: 0, index: 0 }, -1)).toBe(draft)
  })

  it('adds picked targets once and removes components and groups', () => {
    const draft = draftFromGroups(groups)
    const added = addComponents(draft, 1, [
      { target: monitor('1'), name: 'API' },
      { target: monitor('3'), name: 'Mail' },
      { target: monitor('3'), name: 'Mail' },
    ])
    expect(names(added)).toEqual([['API', 'Shop'], ['Phone', 'Mail']])
    expect(names(removeComponent(added, { group: 1, index: 0 }))).toEqual([['API', 'Shop'], ['Mail']])
    expect(names(removeGroup(added, 0))).toEqual([['Phone', 'Mail']])
    const fresh = addComponents([], 0, [{ target: component('9'), name: 'Phone' }])
    expect(fresh).toHaveLength(1)
    expect(fresh[0]?.id).toBeUndefined()
    expect(addGroup(fresh, 'More').map((group) => group.name)).toEqual(['', 'More'])
  })
})

describe('layout editor', () => {
  it('saves with the revision the draft is based on and follows the answer', async () => {
    const page = shallowRef<PageDetail | null>(detail(7))
    const send = vi.fn(async () => detail(8, [groups[1]!]))
    const editor = useLayoutEditor(page, send)

    editor.draft.value = moveGroup(editor.draft.value, 1, 0)
    expect(editor.dirty.value).toBe(true)
    await editor.save()

    expect(send).toHaveBeenCalledWith(expect.objectContaining({ page: 'p1', expected_revision: 7 }))
    expect(names(editor.draft.value)).toEqual([['Phone']])
    expect(editor.dirty.value).toBe(false)
  })

  it('follows live changes until edited, then keeps the draft', async () => {
    const page = shallowRef<PageDetail | null>(detail(7))
    const editor = useLayoutEditor(page, vi.fn())

    page.value = detail(8, [groups[0]!])
    await nextTick()
    expect(names(editor.draft.value)).toEqual([['API', 'Shop']])

    editor.draft.value = removeGroup(editor.draft.value, 0)
    page.value = detail(9)
    await nextTick()
    expect(editor.draft.value).toEqual([])
    expect(editor.outdated.value).toBe(true)
  })

  it('keeps the draft on a revision conflict until reloaded', async () => {
    const page = shallowRef<PageDetail | null>(detail(7))
    const editor = useLayoutEditor(page, vi.fn(async () => Promise.reject(stale())))

    editor.draft.value = removeGroup(editor.draft.value, 1)
    page.value = detail(8)
    await nextTick()
    await editor.save()

    expect(editor.conflict.value).toBe(true)
    expect(names(editor.draft.value)).toEqual([['API', 'Shop']])
    editor.reload()
    expect(editor.conflict.value).toBe(false)
    expect(names(editor.draft.value)).toEqual([['API', 'Shop'], ['Phone']])
    expect(editor.dirty.value).toBe(false)
  })
})
