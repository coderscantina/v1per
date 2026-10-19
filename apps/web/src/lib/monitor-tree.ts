import type { MonitorId, MonitorSummary, NodeId, TreeNode } from '~/generated'
import { presenceOf, presenceRank, type Presence } from '~/lib/monitor-status'
import { fold, isReadable, type TreeIndex } from '~/lib/node-tree'

/**
 * The visible node tree with its monitors, as the shell's tree, the observe dashboard and the
 * orbit read it. Placeholders drop out: the readable nodes without a readable parent become the
 * tops. A monitor sits inside its group when the group is on the same node, as in Kuma.
 *
 * A group monitor named like its node stands for that node: the Kuma import turns each group
 * into a node and keeps a group monitor of the same name on it for the group's own alerting
 * (decision 0007). Such a group is left out here and its members sit on the node directly, so
 * the node does not show twice. Its node's configuration still lists it.
 */
export interface MonitorTree {
  roots: TreeNode[]
  /** Readable children of a node, in order. */
  children: (id: NodeId) => TreeNode[]
  /** Monitors on a node that are not inside a group there, by name. */
  monitorsOn: (id: NodeId) => MonitorSummary[]
  /** The members of a group monitor. */
  members: (id: MonitorId) => MonitorSummary[]
  /** Every monitor on a node and below. */
  below: (id: NodeId) => MonitorSummary[]
  /** The worst presence on a node and below; `unknown` without monitors. */
  rollup: (id: NodeId) => Presence
  monitor: (id: MonitorId) => MonitorSummary | undefined
}

const NONE: never[] = []

/** The worst presence of a list, in the order a list sorted by status shows them. */
export const worstOf = (monitors: readonly MonitorSummary[]): Presence =>
  monitors.reduce<Presence>((worst, monitor) => {
    const presence = presenceOf(monitor)
    return presenceRank(presence) < presenceRank(worst) ? presence : worst
  }, 'unknown')

/** Readable nodes without a readable parent: where the caller's grants start. */
export function topsOf(index: TreeIndex): TreeNode[] {
  return index.order.filter((node) => {
    const parent = node.parent === undefined ? undefined : index.byId.get(node.parent)
    return isReadable(node) && !(parent && isReadable(parent))
  })
}

export function buildMonitorTree(index: TreeIndex, monitors: readonly MonitorSummary[]): MonitorTree {
  const roots = topsOf(index)
  const children = new Map<NodeId, TreeNode[]>()
  for (const [id, list] of index.children) children.set(id, list.filter(isReadable))

  const byId = new Map(monitors.map((monitor) => [monitor.id, monitor]))
  const own = new Map<NodeId, MonitorSummary[]>()
  const on = new Map<NodeId, MonitorSummary[]>()
  const members = new Map<MonitorId, MonitorSummary[]>()
  const append = <K>(map: Map<K, MonitorSummary[]>, key: K, monitor: MonitorSummary) => {
    const list = map.get(key)
    if (list) list.push(monitor)
    else map.set(key, [monitor])
  }
  const standsIn = (monitor: MonitorSummary) => {
    const name = index.byId.get(monitor.node)?.name
    return monitor.type === 'group' && name !== undefined && fold(monitor.name.trim()) === fold(name.trim())
  }
  for (const monitor of monitors) {
    if (standsIn(monitor)) continue
    append(own, monitor.node, monitor)
    const group = monitor.group === undefined ? undefined : byId.get(monitor.group)
    if (group && group.node === monitor.node && !standsIn(group)) append(members, group.id, monitor)
    else append(on, monitor.node, monitor)
  }

  // Depth first puts every node after its parent, so walking it backwards sees children first.
  const below = new Map<NodeId, MonitorSummary[]>()
  const rollup = new Map<NodeId, Presence>()
  for (const node of [...index.order].reverse()) {
    if (!isReadable(node)) continue
    const all = [...(own.get(node.id) ?? NONE), ...(children.get(node.id) ?? NONE).flatMap((child) => below.get(child.id) ?? NONE)]
    below.set(node.id, all)
    rollup.set(node.id, worstOf(all))
  }

  return {
    roots,
    children: (id) => children.get(id) ?? NONE,
    monitorsOn: (id) => on.get(id) ?? NONE,
    members: (id) => members.get(id) ?? NONE,
    below: (id) => below.get(id) ?? NONE,
    rollup: (id) => rollup.get(id) ?? 'unknown',
    monitor: (id) => byId.get(id),
  }
}

/** Down and pending need someone; maintenance, paused and new monitors do not. */
export const isProblem = (presence: Presence): boolean => presence === 'down' || presence === 'pending'

export type TreeRow =
  | { kind: 'node'; key: string; node: TreeNode; depth: number; open: boolean; expandable: boolean; presence: Presence; total: number; down: number }
  | { kind: 'monitor'; key: string; monitor: MonitorSummary; depth: number; open: boolean; expandable: boolean; presence: Presence }

/** Node and monitor IDs share one number space, so open rows are keyed by kind. */
export const nodeKey = (id: NodeId) => `n:${id}`
export const monitorKey = (id: MonitorId) => `m:${id}`

export interface RowOptions {
  /** Keys of the expanded nodes and groups. */
  open: ReadonlySet<string>
  /** Case and accents ignored. A matching node or group shows everything inside it. */
  query: string
  /** Only down and pending monitors, and the nodes and groups that hold them. */
  problems: boolean
}

/**
 * The tree's visible rows in order: each node, then its child nodes, then its monitors. While
 * filtering every kept row is expanded, so the matches show without clicking.
 */
export function treeRows(tree: MonitorTree, options: RowOptions): TreeRow[] {
  const needle = fold(options.query.trim())
  const filtering = needle !== '' || options.problems
  const hit = (name: string | undefined) => needle !== '' && name !== undefined && fold(name).includes(needle)
  const passes = (monitor: MonitorSummary) => !options.problems || isProblem(presenceOf(monitor))

  // `named`: the query is satisfied by this row or one above it.
  const keepMonitor = (monitor: MonitorSummary, named: boolean): boolean => {
    const inside = named || hit(monitor.name)
    return (inside && passes(monitor)) || tree.members(monitor.id).some((member) => keepMonitor(member, inside))
  }
  const keepNode = (node: TreeNode, named: boolean): boolean => {
    const inside = named || hit(node.name)
    if (inside && !options.problems) return true
    return tree.monitorsOn(node.id).some((monitor) => keepMonitor(monitor, inside)) || tree.children(node.id).some((child) => keepNode(child, inside))
  }

  const rows: TreeRow[] = []
  const pushMonitor = (monitor: MonitorSummary, depth: number, named: boolean) => {
    if (filtering && !keepMonitor(monitor, named)) return
    const key = monitorKey(monitor.id)
    const inside = tree.members(monitor.id)
    const open = inside.length > 0 && (filtering || options.open.has(key))
    rows.push({ kind: 'monitor', key, monitor, depth, open, expandable: inside.length > 0, presence: presenceOf(monitor) })
    if (open) for (const member of inside) pushMonitor(member, depth + 1, named || hit(monitor.name))
  }
  const pushNode = (node: TreeNode, depth: number, named: boolean) => {
    if (filtering && !keepNode(node, named)) return
    const key = nodeKey(node.id)
    const kids = tree.children(node.id)
    const own = tree.monitorsOn(node.id)
    const all = tree.below(node.id)
    const expandable = kids.length > 0 || own.length > 0
    const open = expandable && (filtering || options.open.has(key))
    rows.push({
      kind: 'node',
      key,
      node,
      depth,
      open,
      expandable,
      presence: tree.rollup(node.id),
      total: all.length,
      down: all.filter((monitor) => presenceOf(monitor) === 'down').length,
    })
    if (!open) return
    const inside = named || hit(node.name)
    for (const child of kids) pushNode(child, depth + 1, inside)
    for (const monitor of own) pushMonitor(monitor, depth + 1, inside)
  }
  // Without a query every name passes; only the problems filter narrows.
  for (const root of tree.roots) pushNode(root, 0, needle === '')
  return rows
}

/** One satellite of the orbit or tile group of the wall: a child node or a monitor. */
export type ScopeItem = { kind: 'node'; node: TreeNode } | { kind: 'monitor'; monitor: MonitorSummary }

/** What sits directly in a node: its child nodes, then its monitors outside groups. */
export const itemsOf = (tree: MonitorTree, id: NodeId): ScopeItem[] => [
  ...tree.children(id).map((node) => ({ kind: 'node' as const, node })),
  ...tree.monitorsOn(id).map((monitor) => ({ kind: 'monitor' as const, monitor })),
]

/** What sits directly in an item: a node's items, a group's members. */
export const innerItems = (tree: MonitorTree, item: ScopeItem): ScopeItem[] =>
  item.kind === 'node'
    ? itemsOf(tree, item.node.id)
    : tree.members(item.monitor.id).map((monitor) => ({ kind: 'monitor' as const, monitor }))

/** Every monitor an item stands for: a node's whole subtree, a group with all its members. */
export function monitorsIn(tree: MonitorTree, item: ScopeItem): MonitorSummary[] {
  if (item.kind === 'node') return tree.below(item.node.id)
  const all = [item.monitor]
  for (let at = 0; at < all.length; at += 1) {
    const monitor = all[at]
    if (monitor) all.push(...tree.members(monitor.id))
  }
  return all
}

export const itemKey = (item: ScopeItem) => (item.kind === 'node' ? nodeKey(item.node.id) : monitorKey(item.monitor.id))
export const itemName = (item: ScopeItem) => (item.kind === 'node' ? (item.node.name ?? '') : item.monitor.name)
