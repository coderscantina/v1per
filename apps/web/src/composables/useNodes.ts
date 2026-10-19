import { createSharedComposable } from '@vueuse/core'
import { computed, ref, toValue, watch, type MaybeRefOrGetter } from 'vue'
import { useRoute } from 'vue-router'

import { membersResource } from '~/api/resources/members'
import { nodesResource } from '~/api/resources/nodes'
import type { NodeId } from '~/generated'
import { firstReadable, indexTree, isReadable, pathTo } from '~/lib/node-tree'

import { useLiveSnapshot } from './useLive'
import { useSession } from './useSession'

/**
 * The caller's visible tree, live on their `user:<id>` topic: any change to what they see
 * records `tree.changed` there. One snapshot for the whole shell.
 */
export const useNodeTree = createSharedComposable(() => {
  const { account } = useSession()
  const live = useLiveSnapshot({
    key: () => (account.value ? `tree:${account.value.id}` : null),
    load: (_, signal) => nodesResource.tree(signal),
    affects: (event) => event.kind === 'tree.changed',
  })
  const index = computed(() => indexTree(live.data.value?.nodes ?? []))
  return { ...live, index }
})

const LAST_NODE_KEY = 'v1per:last-node'

const readLast = (): NodeId | null => {
  try {
    return window.localStorage.getItem(LAST_NODE_KEY)
  } catch {
    return null
  }
}

/** The node pages outside `/n/:nodeId` (account, instance) keep showing in the shell. */
const lastNodeId = ref<NodeId | null>(typeof window === 'undefined' ? null : readLast())

/**
 * Where the app opens: the last node visited while it is still readable, else the highest node
 * the caller reads. `null` for someone without any role.
 */
export async function startNode(): Promise<NodeId | null> {
  const { nodes } = await nodesResource.tree()
  const index = indexTree(nodes)
  const last = lastNodeId.value ? index.byId.get(lastNodeId.value) : undefined
  return last && isReadable(last) ? last.id : (firstReadable(index)?.id ?? null)
}

/** Node routes keep their section when the picker switches nodes. */
export const nodeRouteNames = [
  'node',
  'node-monitors',
  'node-incidents',
  'node-maintenance',
  'node-members',
  'node-probes',
  'node-channels',
  'node-components',
  'node-pages',
  'node-settings',
] as const
export type NodeRouteName = (typeof nodeRouteNames)[number]
export const isNodeRoute = (name: unknown): name is NodeRouteName => nodeRouteNames.some((entry) => entry === name)

/** Pages of one item belong to their node's list of it. */
const itemSections: Record<string, NodeRouteName> = {
  'node-monitor': 'node-monitors',
  'node-monitor-new': 'node-monitors',
  'node-monitor-edit': 'node-monitors',
  'node-incident': 'node-incidents',
  'node-incident-new': 'node-incidents',
  'node-incident-edit': 'node-incidents',
  'node-maintenance-new': 'node-maintenance',
  'node-maintenance-edit': 'node-maintenance',
  'node-channel': 'node-channels',
  'node-channel-new': 'node-channels',
  'node-channel-edit': 'node-channels',
  'node-page': 'node-pages',
  'node-page-layout': 'node-pages',
  'node-page-access': 'node-pages',
  'node-page-hostnames': 'node-pages',
  'node-page-preview': 'node-pages',
}

/** The section a node switch keeps: the route itself, the list for an item's page, else the overview. */
export const nodeSection = (name: unknown): NodeRouteName => {
  if (isNodeRoute(name)) return name
  return (typeof name === 'string' ? itemSections[name] : undefined) ?? 'node'
}

/**
 * The node the shell is scoped to: the route's `nodeId`, else the last one visited, else the
 * highest node the caller reads. Resolved against the visible tree only, so an ID the caller
 * cannot read resolves to nothing.
 */
export const useCurrentNode = createSharedComposable(() => {
  const route = useRoute()
  const tree = useNodeTree()

  const routeId = computed(() => (typeof route.params.nodeId === 'string' ? route.params.nodeId : null))
  watch(
    routeId,
    (id) => {
      if (!id) return
      lastNodeId.value = id
      try {
        window.localStorage.setItem(LAST_NODE_KEY, id)
      } catch {
        // The choice still applies for this tab.
      }
    },
    { immediate: true }
  )

  const node = computed(() => {
    const index = tree.index.value
    const id = routeId.value ?? lastNodeId.value
    const found = id ? index.byId.get(id) : undefined
    if (found && isReadable(found)) return found
    return routeId.value ? null : firstReadable(index)
  })
  const path = computed(() => (node.value ? pathTo(tree.index.value, node.value.id) : []))

  return { routeId, node, path, tree }
})

/**
 * One node's detail, live on `node:<id>`, which carries only edits of the node and its children. The node's own topic misses changes above it (an
 * ancestor renamed or requiring 2FA), and those reach the caller's tree, so a new tree reads
 * the node again too.
 */
export function useNodeDetail(id: MaybeRefOrGetter<NodeId | null>) {
  const tree = useNodeTree()
  const live = useLiveSnapshot({
    key: () => toValue(id),
    load: (node, signal) => nodesResource.get(node, signal),
    affects: (event) => event.kind.startsWith('node.'),
  })
  watch(
    () => tree.data.value?.revision,
    (next, previous) => {
      if (previous !== undefined && next !== previous && live.data.value) void live.reload()
    }
  )
  return live
}

/**
 * Everyone with access to a node, live on `members:<id>`. `members.changed` marks a grant
 * changed above the node, which reaches everyone below.
 */
export function useNodeMembers(id: MaybeRefOrGetter<NodeId | null>) {
  return useLiveSnapshot({
    key: () => toValue(id),
    load: (node, signal) => membersResource.list(node, signal),
    affects: (event) => event.kind.startsWith('member.') || event.kind === 'members.changed',
  })
}

/** Invitations to a node, live on `members:<id>`. Pass `null` when the caller may not manage members. */
export function useNodeInvitations(id: MaybeRefOrGetter<NodeId | null>) {
  return useLiveSnapshot({
    key: () => toValue(id),
    load: (node, signal) => membersResource.invitations(node, signal),
    affects: (event) => event.kind.startsWith('invitation.') || event.kind === 'member.joined',
  })
}
