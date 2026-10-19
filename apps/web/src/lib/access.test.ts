import { describe, expect, it } from 'vitest'

import { roleCapabilities } from '~/generated/access'
import type { Member, Role } from '~/generated'

import { assignableRoles, grantSource, keyCapabilityOptions, memberControls, moveImpact } from './access'

const member = (user: string, role: Role, grants: Array<[string, Role]>, instance_admin = false): Member => ({
  user,
  name: user,
  email: `${user}@example.test`,
  role,
  grants: grants.map(([node, grantRole]) => ({ node, role: grantRole })),
  instance_admin,
  two_factor_enrolled: false,
})

const controls = (actor: Role, target: Member, self = 'me') =>
  memberControls({ actor, capabilities: roleCapabilities[actor], node: 'n', self, member: target })

describe('capability gating', () => {
  it('hands out only roles below your own, and owners make owners', () => {
    expect(assignableRoles('owner')).toEqual(['owner', 'admin', 'member', 'viewer'])
    expect(assignableRoles('admin')).toEqual(['member', 'viewer'])
    expect(assignableRoles('viewer')).toEqual([])
    expect(assignableRoles(undefined)).toEqual([])
  })

  it('offers role changes and removal only on grants of this node under the rank rule', () => {
    expect(controls('admin', member('a', 'member', [['n', 'member']]))).toEqual({ changeRole: null, remove: null })
    expect(controls('admin', member('a', 'admin', [['n', 'admin']]))).toEqual({ changeRole: 'rank', remove: 'rank' })
    expect(controls('owner', member('a', 'owner', [['n', 'owner']]))).toEqual({ changeRole: null, remove: null })
    expect(controls('admin', member('a', 'member', [['parent', 'member']]))).toEqual({
      changeRole: 'inherited',
      remove: 'inherited',
    })
    expect(controls('viewer', member('a', 'viewer', [['n', 'viewer']]))).toEqual({
      changeRole: 'capability',
      remove: 'capability',
    })
  })

  it('lets you leave but never change your own role', () => {
    expect(controls('viewer', member('me', 'viewer', [['n', 'viewer']]))).toEqual({ changeRole: 'self', remove: null })
  })

  it('names where the effective role comes from', () => {
    expect(grantSource(member('a', 'admin', [['n', 'viewer'], ['p', 'admin']]), 'n')).toEqual({
      kind: 'ancestor',
      node: 'p',
      role: 'admin',
    })
    expect(grantSource(member('a', 'member', [['n', 'member']]), 'n')).toEqual({ kind: 'here', role: 'member' })
    expect(grantSource(member('a', 'owner', [], true), 'n')).toEqual({ kind: 'instance_admin' })
  })

  it('limits key capabilities to what you hold and what keys may carry', () => {
    const options = keyCapabilityOptions(roleCapabilities.member)
    expect(options.filter((option) => option.held).map((option) => option.capability)).toEqual([
      'node.read',
      'monitors.manage',
      'incidents.manage',
      'maintenance.manage',
    ])
    expect(options.map((option) => option.capability)).not.toContain('members.manage')
  })
})

describe('move impact', () => {
  it('lists who gains, loses or changes access, keeping grants that move with the node', () => {
    const admin = member('root', 'owner', [], true)
    const before = [
      admin,
      member('old', 'admin', [['old-parent', 'admin']]),
      member('own', 'viewer', [['n', 'viewer']]),
      member('both', 'member', [['old-parent', 'member']]),
    ]
    const parent = [admin, member('new', 'member', [['new-parent', 'member']]), member('both', 'admin', [['x', 'admin']])]

    const impact = moveImpact(before, parent, 'n')

    expect(impact.gains.map((entry) => [entry.member.user, entry.role])).toEqual([['new', 'member']])
    expect(impact.losses.map((entry) => entry.user)).toEqual(['old'])
    expect(impact.changes.map((entry) => [entry.member.user, entry.from, entry.to])).toEqual([['both', 'member', 'admin']])
  })
})
