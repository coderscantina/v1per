import type { Capability, InvitationState, Role } from '~/generated'
import type { MessageKey } from '~/plugins/i18n'

/** Exhaustive label maps for generated enums. */
export const roleLabels: Record<Role, MessageKey> = {
  owner: 'roles.owner',
  admin: 'roles.admin',
  member: 'roles.member',
  viewer: 'roles.viewer',
}

export const invitationStateLabels: Record<InvitationState, MessageKey> = {
  pending: 'invitations.states.pending',
  used: 'invitations.states.used',
  revoked: 'invitations.states.revoked',
  expired: 'invitations.states.expired',
}

export const capabilityLabels: Record<Capability, MessageKey> = {
  'node.read': 'capabilities.nodeRead',
  'monitors.manage': 'capabilities.monitorsManage',
  'incidents.manage': 'capabilities.incidentsManage',
  'maintenance.manage': 'capabilities.maintenanceManage',
  'pages.manage': 'capabilities.pagesManage',
  'channels.manage': 'capabilities.channelsManage',
  'probes.manage': 'capabilities.probesManage',
  'node.write': 'capabilities.nodeWrite',
  'members.manage': 'capabilities.membersManage',
  'node.move': 'capabilities.nodeMove',
  'node.delete': 'capabilities.nodeDelete',
}
