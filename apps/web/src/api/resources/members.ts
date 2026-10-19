import type { InvitationId, MemberArgs, MembersInviteArgs, MembersUpdateRoleArgs, NodeId } from '~/generated'

import { api } from '..'

/** Who has access to a node, and invitations to it. Both lists follow `members:<id>`. */
export const membersResource = {
  list: (node: NodeId, signal?: AbortSignal) => api.query('members.list', { node }, { signal }),
  invitations: (node: NodeId, signal?: AbortSignal) => api.query('members.invitations', { node }, { signal }),
  invite: (args: MembersInviteArgs) => api.command('members.invite', args),
  revokeInvitation: (invitation: InvitationId) => api.command('members.revokeInvitation', { invitation }),
  updateRole: (args: MembersUpdateRoleArgs) => api.command('members.updateRole', args),
  remove: (args: MemberArgs) => api.command('members.remove', args),
}
