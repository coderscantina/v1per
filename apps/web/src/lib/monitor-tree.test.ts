import { describe, expect, it } from 'vitest'

import { roleCapabilities } from '~/generated/access'
import type { HeartbeatStatus, MonitorSummary, TreeNode } from '~/generated'

import { buildMonitorTree, itemsOf, nodeKey, treeRows, type TreeRow } from './monitor-tree'
import { indexTree } from './node-tree'

const hidden = (id: string, depth: number, parent?: string): TreeNode => ({
  id,
  parent,
  depth,
  position: 0,
  capabilities: [],
  two_factor_required: false,
})
const node = (id: string, name: string, depth: number, parent: string, position = 0): TreeNode => ({
  id,
  parent,
  name,
  depth,
  position,
  role: 'admin',
  capabilities: [...roleCapabilities.admin],
  two_factor_required: false,
})
const monitor = (id: string, name: string, on: string, status?: HeartbeatStatus, group?: string, position = 0): MonitorSummary => ({
  id,
  node: on,
  position,
  name,
  type: 'http',
  active: true,
  tags: [],
  group,
  status,
})
const group = (id: string, name: string, on: string, status?: HeartbeatStatus): MonitorSummary => ({ ...monitor(id, name, on, status), type: 'group' })

/** The autohaus admin's view: a placeholder above, the client with two projects. */
const index = indexTree([
  hidden('1', 0),
  node('7', 'Autohaus', 1, '1'),
  node('8', 'Schmidt', 2, '7'),
  node('9', 'Mötormobile', 2, '7', 1),
])
const tree = buildMonitorTree(index, [
  monitor('20', 'portal', '7', 'up'),
  group('26', 'Schmidt', '8', 'down'),
  monitor('21', 'shop', '8', 'down', '26'),
  monitor('22', 'booking', '8', 'up', '26'),
  group('23', 'group feeds', '9', 'up'),
  monitor('24', 'inventory', '9', 'pending', '23'),
  monitor('25', 'dns', '9', 'up', '23'),
])

const show = (rows: TreeRow[]) => rows.map((row) => `${'  '.repeat(row.depth)}${row.kind === 'node' ? row.node.name : row.monitor.name}`)
const options = { open: new Set<string>(), query: '', problems: false }

describe('monitor tree', () => {
  it('starts at the readable nodes below placeholders', () => {
    expect(tree.roots.map((entry) => entry.id)).toEqual(['7'])
  })

  it('rolls the worst status and every monitor up to each node', () => {
    expect(tree.rollup('7')).toBe('down')
    expect(tree.rollup('9')).toBe('pending')
    expect(tree.below('7').map((entry) => entry.id)).toEqual(['20', '21', '22', '23', '24', '25'])
  })

  it('lifts the members of a group named like its node onto the node, as the Kuma import builds it', () => {
    expect(tree.monitorsOn('8').map((entry) => entry.name)).toEqual(['shop', 'booking'])
    expect(tree.below('8')).toHaveLength(2)
  })

  it('nests monitors in their group on the same node', () => {
    expect(tree.monitorsOn('9').map((entry) => entry.name)).toEqual(['group feeds'])
    expect(tree.members('23').map((entry) => entry.name)).toEqual(['inventory', 'dns'])
    expect(itemsOf(tree, '7').map((item) => (item.kind === 'node' ? item.node.name : item.monitor.name))).toEqual([
      'Schmidt',
      'Mötormobile',
      'portal',
    ])
  })
})

describe('tree rows', () => {
  it('orders monitors by their place on the node, not by name', () => {
    const placed = buildMonitorTree(index, [monitor('30', 'alpha', '9', 'up', undefined, 2), monitor('31', 'beta', '9', 'up', undefined, 0), monitor('32', 'gamma', '9', 'up', undefined, 1)])
    expect(placed.monitorsOn('9').map((entry) => entry.name)).toEqual(['beta', 'gamma', 'alpha'])
  })

  it('shows what is open, children before monitors, with down counts on nodes', () => {
    const rows = treeRows(tree, { ...options, open: new Set([nodeKey('7'), nodeKey('9')]) })
    expect(show(rows)).toEqual(['Autohaus', '  Schmidt', '  Mötormobile', '    group feeds', '  portal'])
    expect(rows[0]).toMatchObject({ kind: 'node', presence: 'down', total: 6, down: 1, open: true })
    expect(rows[3]).toMatchObject({ kind: 'monitor', expandable: true, open: false })
  })

  it('filters by name, ignoring case and accents, and opens the way to each match', () => {
    expect(show(treeRows(tree, { ...options, query: 'INVENT' }))).toEqual(['Autohaus', '  Mötormobile', '    group feeds', '      inventory'])
    expect(show(treeRows(tree, { ...options, query: 'motor' }))).toEqual([
      'Autohaus',
      '  Mötormobile',
      '    group feeds',
      '      inventory',
      '      dns',
    ])
  })

  it('keeps only down and pending monitors and what holds them', () => {
    expect(show(treeRows(tree, { ...options, problems: true }))).toEqual([
      'Autohaus',
      '  Schmidt',
      '    shop',
      '  Mötormobile',
      '    group feeds',
      '      inventory',
    ])
  })
})
