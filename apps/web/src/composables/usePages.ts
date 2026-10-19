import { keepPreviousData, useQuery } from '@tanstack/vue-query'
import { computed, inject, provide, toValue, type ComputedRef, type InjectionKey, type MaybeRefOrGetter } from 'vue'

import { queryKeys } from '~/api/query-keys'
import { pagesResource } from '~/api/resources/pages'
import type { IncidentId, LiveEvent, Locale, NodeId, PageAudience, PageId } from '~/generated'

import { useLiveSnapshot } from './useLive'

/** `pages:<id>` events carry no page ID, so every page event reads again. */
const isPageEvent = (event: LiveEvent) => event.kind.startsWith('page.')

/** A node's status pages, live on `pages:<id>`. */
export function useNodePages(id: MaybeRefOrGetter<NodeId | null>) {
  return useLiveSnapshot({
    key: () => toValue(id),
    load: (node, signal) => pagesResource.list({ node }, signal),
    affects: isPageEvent,
  })
}

/** One page with its layout, share tokens and hostnames, live on its node's `pages:<id>`. */
export function usePageDetail(id: MaybeRefOrGetter<PageId | null>) {
  return useLiveSnapshot({
    key: () => toValue(id),
    load: (page, signal) => pagesResource.get(page, signal),
    affects: isPageEvent,
  })
}

/** The page, live, and whether the caller may change it (`pages.manage` on its node). */
export type PageEditorState = ReturnType<typeof usePageDetail> & { manage: ComputedRef<boolean> }

const pageEditorKey: InjectionKey<PageEditorState> = Symbol('page-editor')

/** The editor frame reads the page once and hands it to its tabs, so they share one snapshot. */
export function providePageEditor(state: PageEditorState) {
  provide(pageEditorKey, state)
}

export function usePageEditor(): PageEditorState {
  const state = inject(pageEditorKey)
  if (!state) throw new Error('usePageEditor needs the page editor frame')
  return state
}

/**
 * A page as `audience` sees it, read when asked and whenever `revision` (the page's) moves. Not
 * streamed: the editor already follows the node's page topic.
 */
export function usePagePreview(
  page: MaybeRefOrGetter<PageId>,
  audience: MaybeRefOrGetter<PageAudience>,
  locale: MaybeRefOrGetter<Locale>,
  revision: MaybeRefOrGetter<number>
) {
  return useQuery({
    queryKey: computed(() => [...queryKeys.pagePreview(toValue(page), toValue(audience), toValue(locale)), toValue(revision)]),
    queryFn: ({ signal }) => pagesResource.preview({ page: toValue(page), audience: toValue(audience), locale: toValue(locale) }, signal),
    placeholderData: keepPreviousData,
    staleTime: 0,
  })
}

/** How many status pages a node and its subtree hold, for the overview, live; `null` until read. */
export function usePageCount(id: MaybeRefOrGetter<NodeId | null>) {
  const live = useLiveSnapshot({
    key: () => toValue(id),
    load: (node, signal) => pagesResource.list({ node, subtree: true }, signal),
    affects: isPageEvent,
  })
  return computed(() => live.data.value?.pages.length ?? null)
}

/**
 * The pages an incident is published to or could be. Publishing records an incident event, so
 * the caller passes the incident snapshot's revision and a change reads this again.
 */
export function useIncidentPublications(incident: MaybeRefOrGetter<IncidentId | null>, revision: MaybeRefOrGetter<number>) {
  return useQuery({
    queryKey: computed(() => [...queryKeys.publications(toValue(incident) ?? ''), toValue(revision)]),
    queryFn: ({ signal }) => pagesResource.publications(toValue(incident) ?? '', signal),
    enabled: () => toValue(incident) !== null,
    placeholderData: keepPreviousData,
    staleTime: 0,
  })
}
