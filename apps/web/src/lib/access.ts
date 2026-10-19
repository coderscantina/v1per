import { keyCapabilities, roles } from '~/generated/access'
import type { Capability, Member, NodeId, Role, UserId } from '~/generated'

/**
 * Client-side mirrors of the capability matrix (`docs/contracts/capability-matrix.md`). They
 * only decide which controls to offer and why one is missing; the server checks every call.
 */

const ranks: Record<Role, number> = { owner: 3, admin: 2, member: 1, viewer: 0 }

export const highestRole = (list: Role[]): Role | null =>
  list.reduce<Role | null>((best, role) => (best === null || ranks[role] > ranks[best] ? role : best), null)

/** The rank rule: roles below the actor's own; owners also make owners. */
export const mayAssign = (actor: Role, role: Role): boolean =>
  ranks[role] < ranks[actor] || (actor === 'owner' && role === 'owner')

/** Roles the actor may hand out, highest first. */
export const assignableRoles = (actor: Role | undefined): Role[] =>
  actor ? roles.filter((role) => mayAssign(actor, role)) : []

export const can = (holder: { capabilities: readonly Capability[] } | null | undefined, capability: Capability) =>
  holder?.capabilities.includes(capability) ?? false

/** Why a control on a member row is missing; `null` when it is offered. */
export type MemberRefusal = 'capability' | 'inherited' | 'self' | 'rank'

export interface MemberControls {
  changeRole: MemberRefusal | null
  remove: MemberRefusal | null
}

/**
 * What the actor may do to `member` on `node`. Only a grant on the node itself can change or
 * go; nobody changes their own role, anyone may remove their own grant.
 */
export function memberControls(input: {
  actor: Role
  capabilities: readonly Capability[]
  node: NodeId
  self: UserId | undefined
  member: Member
}): MemberControls {
  const { actor, capabilities, node, self, member } = input
  const direct = member.grants.some((grant) => grant.node === node)
  const manage = capabilities.includes('members.manage')
  const mine = member.user === self
  const refuse = (forSelf: MemberRefusal | null): MemberRefusal | null => {
    if (!direct) return 'inherited'
    if (mine) return forSelf
    if (!manage) return 'capability'
    if (!mayAssign(actor, member.role)) return 'rank'
    return null
  }
  return { changeRole: refuse('self'), remove: refuse(null) }
}

/** Where a member's effective role comes from. */
export type GrantSource =
  | { kind: 'here'; role: Role }
  | { kind: 'ancestor'; node: NodeId; role: Role }
  | { kind: 'instance_admin' }

export function grantSource(member: Member, node: NodeId): GrantSource {
  const deciding = member.grants.find((grant) => grant.role === member.role)
  if (deciding?.node === node) return { kind: 'here', role: deciding.role }
  if (deciding) return { kind: 'ancestor', node: deciding.node, role: deciding.role }
  return { kind: 'instance_admin' }
}

/** Key capabilities, each with whether the caller holds it on the key's node. */
export const keyCapabilityOptions = (held: readonly Capability[]) =>
  keyCapabilities.map((capability) => ({ capability, held: held.includes(capability) }))

export interface MoveImpact {
  gains: Array<{ member: Member; role: Role }>
  losses: Member[]
  changes: Array<{ member: Member; from: Role; to: Role }>
}

/**
 * Who gains or loses access to a node when it moves under a new parent. Before: the node's
 * member list (grants on the node and its old ancestors). After: the new parent's member list
 * plus grants on the node itself, which move with it. Grants below the node are unaffected.
 */
export function moveImpact(nodeMembers: Member[], parentMembers: Member[], node: NodeId): MoveImpact {
  const before = new Map(nodeMembers.map((member) => [member.user, member]))
  const after = new Map<UserId, { member: Member; role: Role }>()
  for (const member of parentMembers) after.set(member.user, { member, role: member.role })
  for (const member of nodeMembers) {
    const own = member.grants.find((grant) => grant.node === node)?.role
    if (!own) continue
    const inherited = after.get(member.user)?.role
    after.set(member.user, { member, role: highestRole(inherited ? [own, inherited] : [own]) ?? own })
  }
  const impact: MoveImpact = { gains: [], losses: [], changes: [] }
  for (const [user, next] of after) {
    const previous = before.get(user)
    if (!previous) impact.gains.push(next)
    else if (previous.role !== next.role) impact.changes.push({ member: previous, from: previous.role, to: next.role })
  }
  for (const [user, member] of before) if (!after.has(user)) impact.losses.push(member)
  return impact
}
