import { createSharedComposable } from '@vueuse/core'
import { computed } from 'vue'

import { buildMonitorTree, topsOf } from '~/lib/monitor-tree'

import { useMonitorsOf } from './useMonitors'
import { useNodeTree } from './useNodes'

/**
 * Every monitor the caller can read, live, joined to the visible tree: one subtree list per top
 * node the caller holds a grant on. Shared by the shell's tree and the observe dashboard, so the
 * instance is read once.
 */
export const useVisibleMonitors = createSharedComposable(() => {
  const nodes = useNodeTree()
  const tops = computed(() => topsOf(nodes.index.value).map((node) => node.id))
  const lists = useMonitorsOf(tops, true)
  const tree = computed(() => buildMonitorTree(nodes.index.value, lists.value.lists.flatMap((list) => list ?? [])))
  return {
    tree,
    /** Before the tree and every list arrived once. */
    loading: computed(() => !nodes.data.value || lists.value.lists.some((list) => list === null)),
    error: computed(() => nodes.error.value ?? lists.value.error),
    status: computed(() => lists.value.status),
  }
})
