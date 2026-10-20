import type { MonitorId, MonitorsMoveArgs, MonitorSummary, NodeId, TreeNode } from '~/generated'
import { can } from '~/lib/access'
import { monitorsIn, type MonitorTree, type TreeRow } from '~/lib/monitor-tree'
import { moveRefusal, type TreeIndex } from '~/lib/node-tree'

/**
 * Where a row of the shell's tree may be dropped and what the drop asks the server. Only what the
 * caller may do yields a plan; everything else is `null`, so no indicator shows. The server
 * checks every call again.
 */
export type Dragged = { kind: 'node'; id: NodeId } | { kind: 'monitor'; id: MonitorId }

/** A line above or below the row, or the row itself as the new container. */
export type Zone = 'before' | 'after' | 'inside'

export type DropPlan =
  /** `nodes.reorder`: `position` among the parent's children without the node. */
  | { kind: 'reorder'; node: NodeId; position: number }
  /** `nodes.move`, then `nodes.reorder` to `position`; `null` keeps the end the move puts it at. */
  | { kind: 'move'; node: NodeId; parent: NodeId; position: number | null }
  /** `monitors.move`: `position` among every monitor on `node` without the moving ones. */
  | { kind: 'monitor'; args: MonitorsMoveArgs }

export interface DropTarget {
  zone: Zone
  plan: DropPlan
}

export interface DragContext {
  index: TreeIndex
  tree: MonitorTree
  maxDepth: number
}

/** Rows that hold others: nodes and group monitors. */
const nests = (row: TreeRow) => row.kind === 'node' || row.monitor.type === 'group'

/**
 * The zone under the pointer, `fraction` being its height within the row from the top. A row that
 * holds others takes the middle third as "inside"; an expanded one also its lower third, since
 * a line below it would sit above its first child.
 */
export function zoneAt(row: TreeRow, fraction: number): Zone {
  if (!nests(row)) return fraction < 0.5 ? 'before' : 'after'
  if (fraction < 1 / 3) return 'before'
  if (fraction > 2 / 3 && !row.open) return 'after'
  return 'inside'
}

const nodeOf = (context: DragContext, id: NodeId | undefined) => (id === undefined ? undefined : context.index.byId.get(id))

/** Whether a row may be picked up at all. */
export function canDrag(context: DragContext, row: TreeRow): boolean {
  if (row.kind === 'monitor') return can(nodeOf(context, row.monitor.node), 'monitors.manage')
  return can(nodeOf(context, row.node.parent), 'node.write')
}

/** The drop of `dragged` on `row` at `fraction` of its height, or `null` when it is refused. */
export function dropTarget(context: DragContext, dragged: Dragged, row: TreeRow, fraction: number): DropTarget | null {
  if (dragged.kind === 'node') {
    const moving = context.index.byId.get(dragged.id)
    if (!moving || row.kind !== 'node') return null
    const zone = zoneAt(row, fraction)
    const plan = nodeDrop(context, moving, row.node, zone)
    return plan && { zone, plan }
  }
  const moving = context.tree.monitor(dragged.id)
  if (!moving) return null
  // Monitors list after a node's child nodes, so a node row only takes them inside.
  const zone = row.kind === 'node' ? 'inside' : zoneAt(row, fraction)
  const plan = monitorDrop(context, moving, row, zone)
  return plan && { zone, plan }
}

function nodeDrop(context: DragContext, moving: TreeNode, target: TreeNode, zone: Zone): DropPlan | null {
  if (zone === 'inside') return nodeInto(context, moving, target, null)
  const parent = nodeOf(context, target.parent)
  if (!parent || target.id === moving.id) return null
  const siblings = (context.index.children.get(parent.id) ?? []).filter((node) => node.id !== moving.id)
  const position = siblings.findIndex((node) => node.id === target.id) + (zone === 'after' ? 1 : 0)
  return nodeInto(context, moving, parent, position)
}

/** `moving` under `parent` at `position`, `null` for the end. */
function nodeInto(context: DragContext, moving: TreeNode, parent: TreeNode, position: number | null): DropPlan | null {
  const from = nodeOf(context, moving.parent)
  // Leaving a parent, even for another spot in it, takes `node.write` there.
  if (!from || !can(from, 'node.write')) return null
  const siblings = context.index.children.get(parent.id) ?? []
  if (parent.id === from.id) {
    const to = position ?? siblings.length - 1
    return to === siblings.findIndex((node) => node.id === moving.id) ? null : { kind: 'reorder', node: moving.id, position: to }
  }
  if (!can(moving, 'node.move') || moveRefusal(context.index, moving.id, parent, context.maxDepth)) return null
  return { kind: 'move', node: moving.id, parent: parent.id, position: position === null || position >= siblings.length ? null : position }
}

function monitorDrop(context: DragContext, moving: MonitorSummary, row: TreeRow, zone: Zone): DropPlan | null {
  if (!can(nodeOf(context, moving.node), 'monitors.manage')) return null
  const { tree } = context
  const last = (node: NodeId, group: MonitorId | undefined) => siblingsIn(tree, node, group).filter((entry) => entry.id !== moving.id).at(-1)
  if (row.kind === 'node') return monitorAt(context, moving, row.node.id, undefined, last(row.node.id, undefined), 'after')
  const target = row.monitor
  // A group never goes into itself or a member of it.
  if (monitorsIn(tree, { kind: 'monitor', monitor: moving }).some((entry) => entry.id === target.id)) return null
  if (zone === 'inside') return monitorAt(context, moving, target.node, target.id, last(target.node, target.id) ?? target, 'after')
  return monitorAt(context, moving, target.node, groupOf(tree, target), target, zone)
}

/**
 * The group a monitor sits in on its node. That includes a group standing in for the node, whose
 * members show on the node directly, so a monitor dropped among them joins it too.
 */
function groupOf(tree: MonitorTree, monitor: MonitorSummary): MonitorId | undefined {
  const group = monitor.group === undefined ? undefined : tree.monitor(monitor.group)
  return group && group.node === monitor.node ? group.id : undefined
}

/**
 * The monitors of a container as the tree shows them: a group's members, else the node's own,
 * which include the members of a group standing in for the node.
 */
function siblingsIn(tree: MonitorTree, node: NodeId, group: MonitorId | undefined): readonly MonitorSummary[] {
  const shown = group !== undefined && tree.below(node).some((entry) => entry.id === group)
  return shown ? tree.members(group) : tree.monitorsOn(node)
}

/**
 * `moving` into `group` on `node`, right before or after `anchor`, last on the node without one.
 * The server places monitors among every monitor on the node, so `anchor` sets the index there;
 * next to it in that order is next to it in its container too.
 */
function monitorAt(
  context: DragContext,
  moving: MonitorSummary,
  node: NodeId,
  group: MonitorId | undefined,
  anchor: MonitorSummary | undefined,
  side: 'before' | 'after'
): DropPlan | null {
  if (!can(nodeOf(context, node), 'monitors.manage')) return null
  const placed = context.tree.placedOn(node)
  const others = placed.filter((entry) => entry.id !== moving.id)
  const at = anchor ? others.findIndex((entry) => entry.id === anchor.id) : -1
  const position = at === -1 ? others.length : at + (side === 'after' ? 1 : 0)
  const unchanged = placed.findIndex((entry) => entry.id === moving.id) === position && groupOf(context.tree, moving) === group
  return unchanged ? null : { kind: 'monitor', args: { monitor: moving.id, node, group, position } }
}
