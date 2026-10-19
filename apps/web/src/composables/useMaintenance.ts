import { keepPreviousData, useQuery } from '@tanstack/vue-query'
import { refDebounced } from '@vueuse/core'
import { computed, toValue, type MaybeRefOrGetter } from 'vue'

import { queryKeys } from '~/api/query-keys'
import { maintenanceResource } from '~/api/resources/maintenance'
import type { LiveEvent, MaintenanceId, MaintenanceSchedule, NodeId } from '~/generated'

import { useLiveSnapshot } from './useLive'

const isMaintenanceEvent = (event: LiveEvent) => event.kind.startsWith('maintenance.')

/** A node's maintenance windows with their status, live on `maintenance:<id>`. */
export function useNodeMaintenance(id: MaybeRefOrGetter<NodeId | null>) {
  return useLiveSnapshot({
    key: () => toValue(id),
    load: (node, signal) => maintenanceResource.list({ node }, signal),
    affects: isMaintenanceEvent,
  })
}

/** One maintenance with its next windows, live on its node's topic. */
export function useMaintenanceDetail(id: MaybeRefOrGetter<MaintenanceId | null>) {
  return useLiveSnapshot({
    key: () => toValue(id),
    load: (maintenance, signal) => maintenanceResource.get(maintenance, signal),
    affects: (event) => isMaintenanceEvent(event) && (event.maintenance === undefined || event.maintenance === toValue(id)),
  })
}

/** Typing pauses this long before the preview is read. */
const PREVIEW_DEBOUNCE_MS = 400

/**
 * The next windows of the schedule being edited. `null` (an incomplete form) reads nothing. The
 * last answer stays while a new one loads, so the list does not flicker while typing.
 */
export function useMaintenancePreview(schedule: MaybeRefOrGetter<MaintenanceSchedule | null>) {
  const serialized = computed(() => {
    const value = toValue(schedule)
    return value ? JSON.stringify(value) : null
  })
  const settled = refDebounced(serialized, PREVIEW_DEBOUNCE_MS)
  return useQuery({
    queryKey: computed(() => queryKeys.maintenancePreview(settled.value ?? '')),
    queryFn: ({ signal }) => {
      // The key is the schedule itself, so the request reads it back from there.
      const value = JSON.parse(settled.value ?? '{}') as MaintenanceSchedule
      return maintenanceResource.preview(value, signal)
    },
    enabled: () => settled.value !== null,
    placeholderData: keepPreviousData,
    retry: false,
  })
}

/** Maintenance on a node and below, for the overview, live. */
export function useMaintenanceSummary(id: MaybeRefOrGetter<NodeId>) {
  return useLiveSnapshot({
    key: () => toValue(id),
    load: (node, signal) => maintenanceResource.list({ node, subtree: true }, signal),
    affects: isMaintenanceEvent,
  })
}
