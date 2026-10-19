import type { InvitationsAcceptArgs } from '~/generated'

import { api } from '..'

/** Redeeming an invitation link, signed in or signed out. */
export const invitationsResource = {
  preview: (code: string, signal?: AbortSignal) => api.query('invitations.preview', { code }, { signal }),
  accept: (args: InvitationsAcceptArgs) => api.command('invitations.accept', args),
}
