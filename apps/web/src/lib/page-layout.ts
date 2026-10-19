import { computed, ref, shallowRef, watch, type Ref } from 'vue'

import { ApiError } from '~/api'
import type { PageDetail, PageGroup, PageGroupId, PageGroupInput, PageId, PagesUpdateLayoutArgs, Target } from '~/generated'

import { targetKey, type TargetKey } from './incidents'

/**
 * The layout editor's state: groups in order, each with its components in order. Edits build a
 * new draft and never touch the one before, so the editor can compare against what it loaded.
 */
export interface DraftComponent {
  key: TargetKey
  target: Target
  /** What the page shows now: the override, else the monitor's or component's own name. */
  name: string
  /** Empty: the page uses the monitor's or component's own name. */
  displayName: string
  internal: boolean
}

export interface DraftGroup {
  /** Stable for the life of the draft: the group's ID, or a local one for a new group. */
  key: string
  id?: PageGroupId
  name: string
  defaultOpen: boolean
  components: DraftComponent[]
}

export type LayoutDraft = DraftGroup[]

/** Where a component sits: its group's index and its index in that group. */
export interface Slot {
  group: number
  index: number
}

let localKeys = 0
const newGroupKey = () => `new-${++localKeys}`

export function draftFromGroups(groups: readonly PageGroup[]): LayoutDraft {
  return groups.map((group) => ({
    key: group.id,
    id: group.id,
    name: group.name,
    defaultOpen: group.default_open,
    components: group.components.map((component) => ({
      key: targetKey(component.target),
      target: component.target,
      name: component.name,
      displayName: component.display_name ?? '',
      internal: component.internal,
    })),
  }))
}

export function toGroupInputs(draft: LayoutDraft): PageGroupInput[] {
  return draft.map((group) => ({
    id: group.id,
    name: group.name.trim(),
    default_open: group.defaultOpen,
    components: group.components.map((component) => ({
      target: component.target,
      display_name: component.displayName.trim() || undefined,
      internal: component.internal,
    })),
  }))
}

/** Moves one entry of a list; `to` is its index afterwards. Out-of-range moves change nothing. */
export function moveItem<T>(list: readonly T[], from: number, to: number): T[] {
  if (from === to || from < 0 || to < 0 || from >= list.length || to >= list.length) return [...list]
  const next = [...list]
  const [item] = next.splice(from, 1)
  if (item !== undefined) next.splice(to, 0, item)
  return next
}

export const moveGroup = (draft: LayoutDraft, from: number, to: number): LayoutDraft => moveItem(draft, from, to)

/**
 * Moves a component to `to`, in its own group or another. `to.index` is its position in the
 * target group afterwards and may equal that group's length to append.
 */
export function moveComponent(draft: LayoutDraft, from: Slot, to: Slot): LayoutDraft {
  const source = draft[from.group]
  const component = source?.components[from.index]
  const target = draft[to.group]
  if (!component || !target) return draft
  const removed = draft.map((group, index) =>
    index === from.group ? { ...group, components: group.components.filter((_, at) => at !== from.index) } : group
  )
  return removed.map((group, index) => {
    if (index !== to.group) return group
    const components = [...group.components]
    components.splice(Math.min(Math.max(to.index, 0), components.length), 0, component)
    return { ...group, components }
  })
}

/**
 * One step up (-1) or down (+1) for the keyboard buttons. At a group's edge the component moves
 * to the end of the group above or the start of the group below; at the page's edge it stays.
 */
export function stepComponent(draft: LayoutDraft, from: Slot, delta: -1 | 1): LayoutDraft {
  const group = draft[from.group]
  if (!group) return draft
  const index = from.index + delta
  if (index >= 0 && index < group.components.length) return moveComponent(draft, from, { group: from.group, index })
  const neighbour = from.group + delta
  const next = draft[neighbour]
  if (!next) return draft
  return moveComponent(draft, from, { group: neighbour, index: delta < 0 ? next.components.length : 0 })
}

/** Whether a component can step in `delta`'s direction. */
export function canStep(draft: LayoutDraft, from: Slot, delta: -1 | 1): boolean {
  const group = draft[from.group]
  if (!group) return false
  const index = from.index + delta
  if (index >= 0 && index < group.components.length) return true
  return draft[from.group + delta] !== undefined
}

export function addGroup(draft: LayoutDraft, name = ''): LayoutDraft {
  return [...draft, { key: newGroupKey(), name, defaultOpen: true, components: [] }]
}

/** Removes a group and the components in it. */
export const removeGroup = (draft: LayoutDraft, index: number): LayoutDraft => draft.filter((_, at) => at !== index)

export const updateGroup = (draft: LayoutDraft, index: number, patch: Partial<Pick<DraftGroup, 'name' | 'defaultOpen'>>): LayoutDraft =>
  draft.map((group, at) => (at === index ? { ...group, ...patch } : group))

/** Targets already on the page; a page shows each monitor or component once. */
export const placedKeys = (draft: LayoutDraft): Set<TargetKey> =>
  new Set(draft.flatMap((group) => group.components.map((component) => component.key)))

/**
 * Appends picked targets to a group, skipping ones already on the page. Without a group the
 * page gets one first.
 */
export function addComponents(draft: LayoutDraft, group: number, picks: ReadonlyArray<{ target: Target; name: string }>): LayoutDraft {
  const base = draft.length === 0 ? addGroup(draft) : draft
  const at = draft.length === 0 ? 0 : group
  const placed = placedKeys(base)
  const fresh: DraftComponent[] = []
  for (const pick of picks) {
    const key = targetKey(pick.target)
    if (placed.has(key)) continue
    placed.add(key)
    fresh.push({ key, target: pick.target, name: pick.name, displayName: '', internal: false })
  }
  return base.map((entry, index) => (index === at ? { ...entry, components: [...entry.components, ...fresh] } : entry))
}

export const removeComponent = (draft: LayoutDraft, from: Slot): LayoutDraft =>
  draft.map((group, index) =>
    index === from.group ? { ...group, components: group.components.filter((_, at) => at !== from.index) } : group
  )

export const updateComponent = (
  draft: LayoutDraft,
  slot: Slot,
  patch: Partial<Pick<DraftComponent, 'displayName' | 'internal'>>
): LayoutDraft =>
  draft.map((group, index) =>
    index === slot.group
      ? { ...group, components: group.components.map((component, at) => (at === slot.index ? { ...component, ...patch } : component)) }
      : group
  )

const fingerprint = (draft: LayoutDraft) => JSON.stringify(toGroupInputs(draft))

type SendLayout = (args: PagesUpdateLayoutArgs) => Promise<PageDetail>

/**
 * The editor around a page's stored layout. Until the draft changes it follows the page as it
 * updates live. Saving sends the revision the draft is based on; when someone else saved first
 * the server refuses with `stale_revision`, the draft stays, and `reload` replaces it with the
 * current layout.
 */
export function useLayoutEditor(detail: Readonly<Ref<PageDetail | null>>, send: SendLayout) {
  const draft = shallowRef<LayoutDraft>([])
  const loaded = shallowRef<LayoutDraft>([])
  const basedOn = ref<number | null>(null)
  const page = ref<PageId | null>(null)
  const saving = ref(false)
  const error = shallowRef<unknown>(null)

  const dirty = computed(() => fingerprint(draft.value) !== fingerprint(loaded.value))
  const conflict = computed(() => error.value instanceof ApiError && error.value.code === 'stale_revision')

  const load = (from: PageDetail) => {
    const next = draftFromGroups(from.groups)
    loaded.value = next
    draft.value = next
    basedOn.value = from.page.revision
    page.value = from.page.id
    error.value = null
  }

  watch(
    detail,
    (current) => {
      if (current && (!dirty.value || current.page.id !== page.value)) load(current)
    },
    { immediate: true }
  )

  /** Someone else changed the layout since this draft was loaded. */
  const outdated = computed(() => detail.value !== null && basedOn.value !== null && detail.value.page.revision !== basedOn.value)

  const save = async () => {
    if (page.value === null || basedOn.value === null || saving.value) return
    saving.value = true
    error.value = null
    try {
      load(await send({ page: page.value, expected_revision: basedOn.value, groups: toGroupInputs(draft.value) }))
    } catch (failure) {
      error.value = failure
    } finally {
      saving.value = false
    }
  }

  /** Drops the draft for the layout as it is now. */
  const reload = () => {
    if (detail.value) load(detail.value)
  }

  return { draft, dirty, outdated, conflict, saving, error, save, reload }
}
