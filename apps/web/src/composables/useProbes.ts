import { useDocumentVisibility, useIntervalFn } from '@vueuse/core'
import { toValue, type MaybeRefOrGetter } from 'vue'

import { probesResource } from '~/api/resources/probes'
import type { NodeId } from '~/generated'

import { useLiveSnapshot } from './useLive'

/** Agents report health with every poll (about every 25 s) without an event, so it is read this often. */
export const PROBE_HEALTH_REFRESH_MS = 30_000

const isProbeEvent = (event: { kind: string }) => event.kind.startsWith('probe.')

/** Reads a live snapshot again on an interval while the tab is visible. */
function refreshWhileVisible(live: { data: { value: unknown }; loading: { value: boolean }; reload: () => unknown }) {
  const visibility = useDocumentVisibility()
  useIntervalFn(() => {
    if (visibility.value === 'visible' && live.data.value && !live.loading.value) void live.reload()
  }, PROBE_HEALTH_REFRESH_MS)
}

/** Probes registered on the node itself with their health, live on `probes:<id>`. */
export function useNodeProbes(id: MaybeRefOrGetter<NodeId | null>) {
  const live = useLiveSnapshot({
    key: () => toValue(id),
    load: (node, signal) => probesResource.list(node, signal),
    affects: isProbeEvent,
  })
  refreshWhileVisible(live)
  return live
}

/**
 * Probes a monitor on the node may use: the node's own and its ancestors', nearest first. Events
 * of an ancestor's probes go to that node's topic, so this is read again on an interval too.
 */
export function useAvailableProbes(id: MaybeRefOrGetter<NodeId | null>) {
  const live = useLiveSnapshot({
    key: () => toValue(id),
    load: (node, signal) => probesResource.available(node, signal),
    affects: isProbeEvent,
  })
  refreshWhileVisible(live)
  return live
}
