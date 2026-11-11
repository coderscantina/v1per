import type { ErrorCode } from '~/generated'

import type { MessageKey } from './messages'

/** The words for a failed subscribe request. */
export function subscribeError(code: ErrorCode | 'network'): MessageKey {
  switch (code) {
    case 'invalid_arguments':
    case 'malformed_request':
      return 'error.email'
    case 'rate_limited':
      return 'error.rateLimited'
    case 'origin_rejected':
      return 'error.origin'
    case 'network':
      return 'error.network'
    default:
      return 'error.unknown'
  }
}

/** The words for a failed call with a subscriber's link. A token the server does not know has expired or was removed. */
export function linkError(code: ErrorCode | 'network'): MessageKey {
  return code === 'invalid_credentials' ? 'subscribe.expired' : subscribeError(code)
}
