import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import type { TopicFrame } from '~/generated'

import { api } from '.'
import { closeAllLive, joinLive, REOPEN_DELAY_MS, TOPICS_PER_STREAM, type LiveListener } from './live'

const subscribed = (topic: string, seq = 1): TopicFrame => ({
  topic,
  kind: 'subscribed',
  head: `${topic}@${seq}`,
  cursor: `${topic}@${seq}`,
})

/** Streams the test drives: `emit` sends a frame on the latest stream, `end` ends it. */
function fakeStreams() {
  const closes: Array<ReturnType<typeof vi.fn>> = []
  const sinks: Array<{ data: (text: string) => void; end: () => void }> = []
  const subscribe = vi.spyOn(api, 'subscribe').mockImplementation((_topics, data, end) => {
    const close = vi.fn()
    closes.push(close)
    sinks.push({ data, end })
    return close
  })
  return {
    subscribe,
    closes,
    /** The topic entries of each opened stream. */
    opened: () => subscribe.mock.calls.map((call) => call[0]),
    emit: (frame: TopicFrame) => sinks.at(-1)?.data(JSON.stringify(frame)),
    end: () => sinks.at(-1)?.end(),
  }
}

function listener() {
  const frames: TopicFrame[] = []
  const dropped = vi.fn()
  const member: LiveListener = { frame: (frame) => frames.push(frame), dropped }
  return { member, frames, dropped }
}

const settle = () => vi.advanceTimersByTime(REOPEN_DELAY_MS)

describe('the live stream', () => {
  beforeEach(() => vi.useFakeTimers())
  afterEach(() => {
    closeAllLive()
    vi.restoreAllMocks()
    vi.useRealTimers()
  })

  it('carries every topic on one stream and routes frames to their listeners', () => {
    const { opened, emit } = fakeStreams()
    const [a, b, c] = [listener(), listener(), listener()]
    joinLive('instance', 'instance@1', a.member)
    joinLive('instance', 'instance@1', b.member)
    joinLive('node:1', 'node:1@4', c.member)
    settle()

    expect(opened()).toEqual([['instance@1', 'node:1@4']])
    emit(subscribed('instance'))
    emit({ topic: 'node:1', kind: 'events', cursor: 'node:1@5', events: [] })
    expect(a.frames.map((frame) => frame.kind)).toEqual(['subscribed'])
    expect(b.frames.map((frame) => frame.kind)).toEqual(['subscribed'])
    expect(c.frames.map((frame) => frame.kind)).toEqual(['events'])
  })

  it('reopens once per settled change with each topic from its last cursor', () => {
    const { opened, closes, emit } = fakeStreams()
    const leaveInstance = joinLive('instance', 'instance@1', listener().member)
    settle()
    emit({ topic: 'instance', kind: 'events', cursor: 'instance@3', events: [] })

    joinLive('node:1', 'node:1@2', listener().member)
    joinLive('node:2', 'node:2@7', listener().member)
    settle()
    expect(opened()).toEqual([['instance@1'], ['instance@3', 'node:1@2', 'node:2@7']])
    expect(closes[0]).toHaveBeenCalledTimes(1)

    leaveInstance()
    settle()
    expect(opened().at(-1)).toEqual(['node:1@2', 'node:2@7'])
  })

  it('keeps the stream when a view moves to a newer cursor of a topic it already follows', () => {
    const { opened } = fakeStreams()
    const leave = joinLive('instance', 'instance@1', listener().member)
    settle()
    joinLive('instance', 'instance@2', listener().member)
    leave()
    settle()
    expect(opened()).toHaveLength(1)
  })

  it('ends only the topic the server ends, and picks it up again from a fresh snapshot', () => {
    const { opened, closes, emit } = fakeStreams()
    const [a, b] = [listener(), listener()]
    joinLive('instance', 'instance@1', a.member)
    joinLive('node:1', 'node:1@1', b.member)
    settle()
    emit(subscribed('instance'))
    emit(subscribed('node:1'))

    emit({ topic: 'node:1', kind: 'resync', cursor: 'node:1@3', reason: 'unavailable' })
    expect(b.frames.at(-1)?.kind).toBe('resync')
    settle()
    expect(closes[0]).not.toHaveBeenCalled()

    emit({ topic: 'instance', kind: 'events', cursor: 'instance@2', events: [] })
    expect(a.frames.at(-1)?.kind).toBe('events')
    expect(b.frames.at(-1)?.kind).toBe('resync')

    joinLive('node:1', 'node:1@3', listener().member)
    settle()
    expect(opened().at(-1)).toEqual(['instance@2', 'node:1@3'])
  })

  it('closes quietly when the server ended every topic', () => {
    const { opened, emit, end } = fakeStreams()
    const one = listener()
    joinLive('instance', 'instance@1', one.member)
    settle()
    emit({ topic: 'instance', kind: 'expired', head: 'instance@1' })
    end()
    vi.runAllTimers()

    expect(one.dropped).not.toHaveBeenCalled()
    expect(opened()).toHaveLength(1)
  })

  it('reconnects every topic from its last cursor with a doubling delay', () => {
    const { opened, emit, end } = fakeStreams()
    const [a, b] = [listener(), listener()]
    joinLive('instance', 'instance@1', a.member)
    joinLive('node:1', 'node:1@1', b.member)
    settle()
    emit({ topic: 'instance', kind: 'events', cursor: 'instance@4', events: [] })

    end()
    expect(a.dropped).toHaveBeenCalledTimes(1)
    expect(b.dropped).toHaveBeenCalledTimes(1)
    const late = listener()
    joinLive('instance', 'instance@1', late.member)
    expect(late.dropped).toHaveBeenCalledTimes(1)

    vi.advanceTimersByTime(1000)
    expect(opened().at(-1)).toEqual(['instance@4', 'node:1@1'])
    end()
    vi.advanceTimersByTime(1000)
    expect(opened()).toHaveLength(2)
    vi.advanceTimersByTime(1000)
    expect(opened()).toHaveLength(3)
  })

  it('splits more topics than one stream takes across streams', () => {
    const { opened } = fakeStreams()
    for (let id = 0; id <= TOPICS_PER_STREAM; id++) joinLive(`node:${id}`, `node:${id}@1`, listener().member)
    settle()
    expect(opened().map((topics) => topics.length)).toEqual([TOPICS_PER_STREAM, 1])
  })
})
