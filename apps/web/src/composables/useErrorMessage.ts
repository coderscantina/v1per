import { ApiError, NetworkError } from '~/api'
import type { ErrorCode, FieldErrorCode } from '~/generated'
import { useI18n, type MessageKey } from '~/plugins/i18n'

/** Exhaustive: a new generated code is a compile error until it has copy. */
const errorMessages: Record<ErrorCode, MessageKey> = {
  unauthenticated: 'errors.unauthenticated',
  invalid_credentials: 'errors.invalid_credentials',
  second_factor_required: 'errors.second_factor_required',
  second_factor_invalid: 'errors.second_factor_invalid',
  second_factor_enrollment_required: 'errors.second_factor_enrollment_required',
  reauthentication_required: 'errors.reauthentication_required',
  not_found: 'errors.not_found',
  forbidden: 'errors.forbidden',
  operation_not_granted: 'errors.operation_not_granted',
  grant_exceeds_principal: 'errors.grant_exceeds_principal',
  origin_rejected: 'errors.origin_rejected',
  stale_revision: 'errors.stale_revision',
  idempotency_conflict: 'errors.idempotency_conflict',
  node_not_empty: 'errors.node_not_empty',
  malformed_request: 'errors.malformed_request',
  invalid_arguments: 'errors.invalid_arguments',
  payload_too_large: 'errors.payload_too_large',
  rate_limited: 'errors.rate_limited',
  query_timeout: 'errors.query_timeout',
  internal: 'errors.internal',
}

const fieldMessages: Record<FieldErrorCode, MessageKey> = {
  required: 'fieldErrors.required',
  too_short: 'fieldErrors.too_short',
  too_long: 'fieldErrors.too_long',
  too_many: 'fieldErrors.too_many',
  invalid_format: 'fieldErrors.invalid_format',
  invalid_value: 'fieldErrors.invalid_value',
}

/** Translated copy for any thrown value: API codes, network loss, or the unexpected. */
export function useErrorMessage() {
  const { t } = useI18n()

  const message = (error: unknown): string => {
    if (error instanceof ApiError) return t(errorMessages[error.code])
    if (error instanceof NetworkError) return t('errors.network')
    return t('errors.internal')
  }

  /** One field problem, translated: from the server or found before sending. */
  const fieldProblem = (problem: { code: FieldErrorCode; limit?: number }): string =>
    t(fieldMessages[problem.code], { limit: problem.limit ?? '' })

  /** The first problem with one field, translated, for `invalid_arguments`. */
  const fieldMessage = (error: unknown, field: string): string | null => {
    if (!(error instanceof ApiError)) return null
    const first = error.body.errors?.[field]?.[0]
    return first ? fieldProblem(first) : null
  }

  return { message, fieldMessage, fieldProblem }
}
