import { describe, expect, it, vi } from 'vitest'

import { api } from '~/api'
import { joinLive, REOPEN_DELAY_MS } from '~/api/live'
import { queryKeys } from '~/api/query-keys'
import type { Account } from '~/generated'
import { queryClient } from '~/plugins/vue-query'

import { applyLogin, forgetEverything } from './useSession'

const account: Account = { id: 'u1', email: 'a@example.test', name: 'A', instance_admin: true }

describe('session state', () => {
  it('drops every cached result and closes every live stream when forgetting', () => {
    queryClient.setQueryData(['instance.info'], { name: 'x' })
    vi.useFakeTimers()
    const closed = vi.fn()
    const subscribe = vi.spyOn(api, 'subscribe').mockImplementation(() => closed)
    const dropped = vi.fn()
    joinLive('instance', 'instance@1', { frame: () => {}, dropped })
    joinLive('user:u1', 'user:u1@1', { frame: () => {}, dropped })
    vi.advanceTimersByTime(REOPEN_DELAY_MS)

    forgetEverything()
    vi.runAllTimers()

    expect(queryClient.getQueryCache().getAll()).toHaveLength(0)
    expect(subscribe).toHaveBeenCalledTimes(1)
    expect(closed).toHaveBeenCalledTimes(1)
    expect(dropped).not.toHaveBeenCalled()
    vi.restoreAllMocks()
    vi.useRealTimers()
  })

  it('replaces everything cached with the account that signed in', () => {
    queryClient.setQueryData(['instance.info'], { name: 'x' })

    expect(applyLogin({ status: 'signed_in', account })).toBe('signed_in')

    expect(queryClient.getQueryData(['instance.info'])).toBeUndefined()
    expect(queryClient.getQueryData(queryKeys.session)).toEqual({ account, enrollment_only: false })
  })

  it('keeps the cache through a second-factor challenge and limits an enrollment session', () => {
    queryClient.setQueryData(['instance.info'], { name: 'x' })

    expect(applyLogin({ status: 'second_factor_required' })).toBe('challenge')
    expect(queryClient.getQueryData(['instance.info'])).toEqual({ name: 'x' })

    expect(applyLogin({ status: 'enrollment_required', account })).toBe('enroll')
    expect(queryClient.getQueryData(queryKeys.session)).toEqual({ account, enrollment_only: true })
  })
})
