import { useInfiniteQuery, useQuery } from '@tanstack/vue-query'
import { computed, toValue, type MaybeRefOrGetter } from 'vue'

import { queryKeys } from '~/api/query-keys'
import { importsResource } from '~/api/resources/imports'
import type { NodeId } from '~/generated'

/**
 * Kuma imports for instance admins. An import runs from the command line while the server is
 * stopped, so these are read once per visit rather than followed.
 */
export function useImports() {
  return useQuery({
    queryKey: queryKeys.imports,
    queryFn: ({ signal }) => importsResource.list(signal),
  })
}

/** The import into one node: its steps and, once complete, the reconciliation manifest. */
export function useImportRun(node: MaybeRefOrGetter<NodeId | null>) {
  return useQuery({
    queryKey: computed(() => queryKeys.importRun(toValue(node) ?? '')),
    queryFn: ({ signal }) => importsResource.get(toValue(node) ?? '', signal),
    enabled: () => toValue(node) !== null,
    retry: false,
  })
}

/** Inventory rows read per page. */
const INVENTORY_PAGE = 100

/** What the import into one node did not map, or mapped with a loss, in source order. */
export function useImportInventory(node: MaybeRefOrGetter<NodeId | null>) {
  return useInfiniteQuery({
    queryKey: computed(() => queryKeys.importInventory(toValue(node) ?? '')),
    queryFn: ({ pageParam, signal }: { pageParam: string | undefined; signal: AbortSignal }) =>
      importsResource.inventory({ node: toValue(node) ?? '', cursor: pageParam, limit: INVENTORY_PAGE }, signal),
    initialPageParam: undefined as string | undefined,
    getNextPageParam: (page) => page.next_cursor,
    enabled: () => toValue(node) !== null,
    retry: false,
  })
}
