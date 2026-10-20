import { describe, expect, it } from 'vitest'

import { roleCapabilities } from '~/generated/access'
import type { MonitorSummary, Role, TreeNode } from '~/generated'

import { buildMonitorTree, treeRows, type TreeRow } from './monitor-tree'
import { indexTree } from './node-tree'
import { canDrag, dropTarget, zoneAt, type DragContext } from './tree-drag'

const node = (id: string, name: string, depth: number, parent?: string, position = 0, role: Role = 'owner'): TreeNode => ({
  id,
  parent,
  name,
  depth,
  position,
  role,
  capabilities: [...roleCapabilities[role]],
  two_factor_required: false,
})
const monitor = (id: string, name: string, on: string, position: number, group?: string, type: MonitorSummary['type'] = 'http'): MonitorSummary => ({
  id,
  node: on,
  position,
  name,
  type,
  active: true,
  tags: [],
  group,
})

/** Autohaus, owned, with two projects; Schmidt holds Werkstatt; a viewer-only client beside them. */
const index = indexTree([
  node('7', 'Autohaus', 0),
  node('8', 'Schmidt', 1, '7'),
  node('10', 'Werkstatt', 2, '8'),
  node('9', 'Mötormobile', 1, '7', 1),
  node('11', 'Gast', 1, '7', 2, 'viewer'),
])
const tree = buildMonitorTree(index, [
  monitor('20', 'portal', '7', 0),
  monitor('23', 'feeds', '9', 0, undefined, 'group'),
  monitor('24', 'inventory', '9', 1, '23'),
  monitor('25', 'dns', '9', 2, '23'),
  monitor('26', 'api', '9', 3),
  monitor('27', 'wlan', '11', 0),
])
const context: DragContext = { index, tree, maxDepth: 6 }
const all = new Set(['n:7', 'n:8', 'n:9', 'n:11', 'm:23'])
const rows = treeRows(tree, { open: all, query: '', problems: false })
const row = (key: string): TreeRow => {
  const found = rows.find((entry) => entry.key === key)
  if (!found) throw new Error(key)
  return found
}
const nodeDrag = (id: string) => ({ kind: 'node' as const, id })
const monitorDrag = (id: string) => ({ kind: 'monitor' as const, id })
const TOP = 0.1
const MIDDLE = 0.5
const BOTTOM = 0.9

describe('tree drag', () => {
  it('splits rows that hold others in thirds, leaf rows in halves', () => {
    expect([TOP, MIDDLE, BOTTOM].map((at) => zoneAt(row('n:10'), at))).toEqual(['before', 'inside', 'after'])
    expect([TOP, BOTTOM].map((at) => zoneAt(row('m:24'), at))).toEqual(['before', 'after'])
    // Below an expanded row is its first child, so the lower third drops inside.
    expect(zoneAt(row('n:8'), BOTTOM)).toBe('inside')
  })

  it('reorders a node among its siblings by its index without itself', () => {
    expect(dropTarget(context, nodeDrag('9'), row('n:8'), TOP)?.plan).toEqual({ kind: 'reorder', node: '9', position: 0 })
    expect(dropTarget(context, nodeDrag('8'), row('n:11'), TOP)?.plan).toEqual({ kind: 'reorder', node: '8', position: 1 })
    expect(dropTarget(context, nodeDrag('8'), row('n:9'), TOP)).toBeNull()
  })

  it('moves a node under another, then to the spot between its children', () => {
    expect(dropTarget(context, nodeDrag('9'), row('n:8'), MIDDLE)?.plan).toEqual({ kind: 'move', node: '9', parent: '8', position: null })
    expect(dropTarget(context, nodeDrag('9'), row('n:10'), TOP)?.plan).toEqual({ kind: 'move', node: '9', parent: '8', position: 0 })
  })

  it('refuses a node into itself or below it, and the top node', () => {
    expect(dropTarget(context, nodeDrag('8'), row('n:10'), MIDDLE)).toBeNull()
    expect(dropTarget(context, nodeDrag('8'), row('n:8'), MIDDLE)).toBeNull()
    expect(canDrag(context, row('n:7'))).toBe(false)
    expect(dropTarget(context, nodeDrag('7'), row('n:9'), MIDDLE)).toBeNull()
  })

  it('refuses a node where the role does not allow it', () => {
    expect(dropTarget(context, nodeDrag('9'), row('n:11'), MIDDLE)).toBeNull()
    // Admins reorder but do not move.
    const admin = indexTree(index.order.map((entry) => (entry.id === '9' ? node('9', 'Mötormobile', 1, '7', 1, 'admin') : entry)))
    const scoped = { ...context, index: admin }
    expect(dropTarget(scoped, nodeDrag('9'), row('n:8'), TOP)?.plan.kind).toBe('reorder')
    expect(dropTarget(scoped, nodeDrag('9'), row('n:8'), MIDDLE)).toBeNull()
  })

  it('places monitors by their index among every monitor on the node', () => {
    const plan = (id: string, key: string, at: number) => dropTarget(context, monitorDrag(id), row(key), at)?.plan
    expect(plan('25', 'm:24', TOP)).toEqual({ kind: 'monitor', args: { monitor: '25', node: '9', group: '23', position: 1 } })
    // Into a group: after its last member.
    expect(plan('26', 'm:23', MIDDLE)).toEqual({ kind: 'monitor', args: { monitor: '26', node: '9', group: '23', position: 3 } })
    expect(dropTarget(context, monitorDrag('24'), row('n:8'), TOP)).toEqual({ zone: 'inside', plan: { kind: 'monitor', args: { monitor: '24', node: '8', group: undefined, position: 0 } } })
    expect(plan('20', 'n:7', MIDDLE)).toBeUndefined()
  })

  it('refuses a group into its own members, and monitors where the role does not allow managing them', () => {
    expect(dropTarget(context, monitorDrag('23'), row('m:24'), TOP)).toBeNull()
    expect(canDrag(context, row('m:27'))).toBe(false)
    expect(dropTarget(context, monitorDrag('26'), row('n:11'), MIDDLE)).toBeNull()
  })
})
