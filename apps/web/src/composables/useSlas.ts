import { toValue, type MaybeRefOrGetter } from 'vue'

import { slasResource } from '~/api/resources/slas'
import type { LiveEvent, NodeId, SlaId } from '~/generated'

import { useLiveSnapshot } from './useLive'

const isSlaEvent = (event: LiveEvent) => event.kind.startsWith('sla.')

/** A node's SLAs, or with `subtree` those of the node and below, live on `slas:<id>`. */
export function useNodeSlas(id: MaybeRefOrGetter<NodeId | null>, subtree = false) {
  return useLiveSnapshot({
    key: () => toValue(id),
    cache: subtree ? 'slas.list.subtree' : 'slas.list',
    load: (node, signal) => slasResource.list({ node, subtree }, signal),
    affects: isSlaEvent,
  })
}

/** One SLA, live on its node's topic. */
export function useSlaDetail(id: MaybeRefOrGetter<SlaId | null>) {
  return useLiveSnapshot({
    key: () => toValue(id),
    cache: 'slas.get',
    load: (sla, signal) => slasResource.get(sla, signal),
    affects: (event) => isSlaEvent(event) && (event.sla === undefined || event.sla === toValue(id)),
  })
}
