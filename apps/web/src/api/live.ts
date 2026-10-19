import type { TopicFrame } from '~/generated'

import { api } from '.'

/** Receives the frames of a followed topic and learns when the stream carrying it drops. */
export type LiveListener = { frame: (frame: TopicFrame) => void; dropped: () => void }

const RETRY_FIRST_MS = 1000
const RETRY_MAX_MS = 30_000
/** How long topic changes gather before the stream reopens, so a screen mounting many panels reopens once. */
export const REOPEN_DELAY_MS = 50
/** Topics one stream carries (architecture patterns, Topics, cursors and subscriptions). */
export const TOPICS_PER_STREAM = 32

type Followed = {
  listeners: Set<LiveListener>
  /** The cursor the topic resumes from, moved forward by every `events` frame. */
  position: string
  /** The last `subscribed` frame while a stream carries the topic, replayed to listeners that join later. */
  subscribed: TopicFrame | null
}

/** Every topic the tab follows, however many views listen to it. */
const followed = new Map<string, Followed>()

/** The open streams, each with the topics the server has not ended on it. Empty while waiting to retry. */
let streams: Array<{ close: () => void; live: Set<string> }> = []
let timer: ReturnType<typeof setTimeout> | null = null
let retrying = false
let delay = RETRY_FIRST_MS

function closeStreams() {
  const current = streams
  streams = []
  for (const stream of current) stream.close()
}

function schedule(ms: number) {
  if (timer) clearTimeout(timer)
  timer = setTimeout(reopen, ms)
}

/** The followed set changed: reopen once changes settle. A pending retry opens the full set anyway. */
function changed() {
  if (!retrying) schedule(REOPEN_DELAY_MS)
}

/** Whether the open streams carry exactly the followed topics. */
function inSync(): boolean {
  const carried = new Set(streams.flatMap((stream) => [...stream.live]))
  return carried.size === followed.size && [...followed.keys()].every((topic) => carried.has(topic))
}

/** Replaces the open streams with ones for every followed topic, each from its last cursor. */
function reopen() {
  timer = null
  retrying = false
  if (inSync()) return
  closeStreams()
  const topics = [...followed.keys()]
  for (let at = 0; at < topics.length; at += TOPICS_PER_STREAM) open(topics.slice(at, at + TOPICS_PER_STREAM))
}

function open(topics: string[]) {
  const stream = { close: () => {}, live: new Set(topics) }
  streams.push(stream)
  stream.close = api.subscribe(
    topics.map((topic) => followed.get(topic)?.position ?? topic),
    // The server is the type's source: frames are generated from the same registry.
    (data) => receive(stream, JSON.parse(data) as TopicFrame),
    () => ended(stream)
  )
}

function receive(stream: (typeof streams)[number], frame: TopicFrame) {
  if (!streams.includes(stream)) return
  const topic = followed.get(frame.topic)
  switch (frame.kind) {
    case 'subscribed':
      delay = RETRY_FIRST_MS
      if (topic) topic.subscribed = frame
      break
    case 'events':
      if (topic) topic.position = frame.cursor
      break
    case 'resync':
    case 'expired':
      // The server ended this topic only. Its listeners join again from a fresh snapshot.
      stream.live.delete(frame.topic)
      if (topic) followed.delete(frame.topic)
  }
  if (topic) for (const listener of topic.listeners) listener.frame(frame)
}

/** A stream whose topics the server all ended closed as expected; any other end drops every topic. */
function ended(stream: (typeof streams)[number]) {
  if (!streams.includes(stream)) return
  if (![...stream.live].some((topic) => followed.has(topic))) {
    streams = streams.filter((other) => other !== stream)
    if (!inSync()) changed()
    return
  }
  closeStreams()
  for (const topic of followed.values()) {
    topic.subscribed = null
    for (const listener of topic.listeners) listener.dropped()
  }
  retrying = true
  schedule(delay)
  delay = Math.min(delay * 2, RETRY_MAX_MS)
}

/**
 * Follows a topic on the tab's one stream (or one per 32 topics). The first listener of a topic
 * sets where it starts; later ones join at its current position. Adding or removing a topic
 * reopens the stream after a short delay with every topic's last cursor, so nothing is missed or
 * repeated. An unexpected close tells every listener and retries after 1 s, doubling up to 30 s;
 * a `subscribed` frame resets the delay. When the server ends a topic (`expired`, or `resync`
 * because access could not be confirmed), its listeners get that frame and must join again from
 * a fresh snapshot; the other topics go on. Returns the leave function.
 */
export function joinLive(topic: string, cursor: string, listener: LiveListener): () => void {
  let joined = followed.get(topic)
  if (joined) {
    if (joined.subscribed) listener.frame(joined.subscribed)
    else if (retrying) listener.dropped()
  } else {
    joined = { listeners: new Set(), position: cursor, subscribed: null }
    followed.set(topic, joined)
    changed()
  }
  joined.listeners.add(listener)

  const mine = joined
  return () => {
    mine.listeners.delete(listener)
    if (mine.listeners.size > 0 || followed.get(topic) !== mine) return
    followed.delete(topic)
    changed()
  }
}

/** Closes every stream and forgets every topic without reporting errors, so nothing reconnects. */
export function closeAllLive(): void {
  if (timer) clearTimeout(timer)
  timer = null
  retrying = false
  delay = RETRY_FIRST_MS
  followed.clear()
  closeStreams()
}

/**
 * The topic a snapshot's cursor (`<topic>@<seq>`) belongs to. Screens follow the topic their
 * snapshot names, so a query moving to another topic needs no change here.
 */
export function topicOf(cursor: string): string {
  const at = cursor.lastIndexOf('@')
  return at > 0 ? cursor.slice(0, at) : cursor
}
