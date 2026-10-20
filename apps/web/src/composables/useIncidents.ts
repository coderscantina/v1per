import { computed, toValue, watch, type MaybeRefOrGetter } from 'vue'

import { incidentsResource } from '~/api/resources/incidents'
import type { IncidentId, IncidentOrigin, IncidentStatusFilter, LiveEvent, NodeId, Target } from '~/generated'
import { targetKey, type TargetKey } from '~/lib/incidents'

import { useComponentTree } from './useComponents'
import { useLiveSnapshot } from './useLive'
import { useMonitorTree } from './useMonitors'

const isIncidentEvent = (event: LiveEvent) => event.kind.startsWith('incident.')

export interface IncidentFilters {
  status?: IncidentStatusFilter
  origin?: IncidentOrigin
  subtree?: boolean
}

/**
 * A node's incidents, newest first, live on `incidents:<id>`. A filter change reads them from
 * scratch; `limit` grows the page (at most 100) without dropping what is shown.
 */
export function useNodeIncidents(
  id: MaybeRefOrGetter<NodeId | null>,
  filters: MaybeRefOrGetter<IncidentFilters> = {},
  limit: MaybeRefOrGetter<number> = 25
) {
  const live = useLiveSnapshot({
    key: () => {
      const node = toValue(id)
      return node ? `${node}:${JSON.stringify(toValue(filters))}` : null
    },
    cache: 'incidents.list',
    load: (_, signal) => incidentsResource.list({ node: toValue(id) ?? '', ...toValue(filters), limit: toValue(limit) }, signal),
    affects: isIncidentEvent,
  })
  watch(
    () => toValue(limit),
    () => void live.reload()
  )
  return live
}

/** One incident with its affected targets and timeline, live on its node's topic. */
export function useIncidentDetail(id: MaybeRefOrGetter<IncidentId | null>) {
  return useLiveSnapshot({
    key: () => toValue(id),
    cache: 'incidents.get',
    load: (incident, signal) => incidentsResource.get(incident, signal),
    affects: (event) => isIncidentEvent(event) && (event.incident === undefined || event.incident === toValue(id)),
  })
}

export interface TargetOption {
  key: TargetKey
  name: string
  kind: Target['type']
  /** The node it sits on, when below the incident's node. */
  node: NodeId
}

/** The newest open incidents on a node and below, for the overview, live. */
export function useOpenIncidents(id: MaybeRefOrGetter<NodeId>, limit: number) {
  return useLiveSnapshot({
    key: () => toValue(id),
    cache: 'incidents.list.subtree.open',
    load: (node, signal) => incidentsResource.list({ node, status: 'open', subtree: true, limit }, signal),
    affects: isIncidentEvent,
  })
}

/**
 * What an incident on the node may name: monitors and static components on the node and below,
 * monitors first, each by name, live.
 */
export function useIncidentTargets(id: MaybeRefOrGetter<NodeId | null>) {
  const monitors = useMonitorTree(id)
  const components = useComponentTree(id)
  const options = computed<TargetOption[]>(() => [
    ...(monitors.data.value?.monitors ?? []).map((monitor) => ({
      key: targetKey({ type: 'monitor', monitor: monitor.id }),
      name: monitor.name,
      kind: 'monitor' as const,
      node: monitor.node,
    })),
    ...(components.data.value?.components ?? []).map((component) => ({
      key: targetKey({ type: 'component', component: component.id }),
      name: component.name,
      kind: 'component' as const,
      node: component.node,
    })),
  ])
  const loading = computed(() => monitors.loading.value || components.loading.value)
  return { options, loading, error: computed(() => monitors.error.value ?? components.error.value) }
}
