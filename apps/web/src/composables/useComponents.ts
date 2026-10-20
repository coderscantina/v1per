import { toValue, type MaybeRefOrGetter } from 'vue'

import { componentsResource } from '~/api/resources/components'
import type { NodeId } from '~/generated'

import { useLiveSnapshot } from './useLive'

/** A node's static components, live on `components:<id>`. */
export function useNodeComponents(id: MaybeRefOrGetter<NodeId | null>) {
  return useLiveSnapshot({
    key: () => toValue(id),
    cache: 'components.list',
    load: (node, signal) => componentsResource.list({ node }, signal),
    affects: (event) => event.kind.startsWith('component.'),
  })
}

/** Static components on a node and below, for pickers such as incident targets, live. */
export function useComponentTree(id: MaybeRefOrGetter<NodeId | null>) {
  return useLiveSnapshot({
    key: () => toValue(id),
    cache: 'components.list.subtree',
    load: (node, signal) => componentsResource.list({ node, subtree: true }, signal),
    affects: (event) => event.kind.startsWith('component.'),
  })
}
