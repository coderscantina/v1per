import { describe, expect, it } from 'vitest'

import { roleCapabilities } from '~/generated/access'
import type { Role, TreeNode } from '~/generated'

import { firstReadable, indexTree, moveRefusal, pathTo, pickerRows } from './node-tree'

const hidden = (id: string, depth: number, parent?: string, position = 0): TreeNode => ({
  id,
  parent,
  depth,
  position,
  capabilities: [],
  two_factor_required: false,
})
const node = (id: string, name: string, depth: number, parent: string, role: Role, position = 0): TreeNode => ({
  id,
  parent,
  name,
  depth,
  position,
  role,
  capabilities: [...roleCapabilities[role]],
  two_factor_required: false,
})

/** What the autohaus admin sees: two placeholders above, the client and its two projects. */
const autohaus = indexTree([
  hidden('1', 0),
  hidden('4', 1, '1'),
  node('7', 'Autohaus', 2, '4', 'admin'),
  node('8', 'Schmidt', 3, '7', 'admin'),
  node('9', 'Mötormobile', 3, '7', 'admin', 1),
])

describe('node tree picker', () => {
  it('lists the whole tree in order, placeholders included but not readable', () => {
    const rows = pickerRows(autohaus, '')
    expect(rows.map((row) => [row.node.id, row.readable, row.indent])).toEqual([
      ['1', false, 0],
      ['4', false, 1],
      ['7', true, 2],
      ['8', true, 3],
      ['9', true, 3],
    ])
  })

  it('searches names and readable ancestors, ignoring case and accents, never placeholders', () => {
    expect(pickerRows(autohaus, 'motor').map((row) => row.node.id)).toEqual(['9'])
    const client = pickerRows(autohaus, 'AUTOHAUS')
    expect(client.map((row) => row.node.id)).toEqual(['7', '8', '9'])
    expect(client[1]?.trail.map((entry) => entry.name ?? null)).toEqual([null, null, 'Autohaus'])
    expect(pickerRows(autohaus, 'nothing')).toEqual([])
  })

  it('opens at the highest readable node and builds paths with placeholders', () => {
    expect(firstReadable(autohaus)?.id).toBe('7')
    expect(pathTo(autohaus, '8').map((entry) => entry.name ?? '…')).toEqual(['…', '…', 'Autohaus', 'Schmidt'])
    expect(pathTo(autohaus, 'missing')).toEqual([])
  })

  it('offers only valid move targets', () => {
    const owner = indexTree([
      node('1', 'Root', 0, '', 'owner'),
      node('2', 'Clients', 1, '1', 'owner'),
      node('3', 'Autohaus', 2, '2', 'owner'),
      node('4', 'Schmidt', 3, '3', 'owner'),
      node('5', 'Ops', 1, '1', 'viewer', 1),
    ].map((entry) => (entry.id === '1' ? { ...entry, parent: undefined } : entry)))
    const target = (id: string) => owner.byId.get(id) ?? hidden(id, 0)

    expect(moveRefusal(owner, '3', target('1'), 8)).toBeNull()
    expect(moveRefusal(owner, '3', target('2'), 8)).toBe('current')
    expect(moveRefusal(owner, '3', target('3'), 8)).toBe('inside')
    expect(moveRefusal(owner, '3', target('4'), 8)).toBe('inside')
    expect(moveRefusal(owner, '3', target('5'), 8)).toBe('capability')
    expect(moveRefusal(owner, '3', target('1'), 1)).toBe('depth')
    expect(moveRefusal(autohaus, '8', hidden('4', 1, '1'), 8)).toBe('hidden')
  })
})
