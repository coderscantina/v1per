import type { ApiErrorBody, CommandMeta, ErrorCode, SnapshotMeta } from '~/generated'
import type { Commands } from '~/generated/commands'
import { errorCodes } from '~/generated/errors'
import { REQUEST_MARKER_HEADER, REQUEST_MARKER_VALUE } from '~/generated/limits'
import type { Queries } from '~/generated/queries'

export type CommandName = keyof Commands
export type CommandArgs<N extends CommandName> = Commands[N]['args']
export type CommandResult<N extends CommandName> = CommandMeta & Commands[N]['result']

export type QueryName = keyof Queries
export type QueryArgs<N extends QueryName> = Queries[N]['args']
export type QuerySnapshot<N extends QueryName> = SnapshotMeta & Queries[N]['result']

/** A proxy's HTML error page can be arbitrarily large; keep enough to identify it. */
const MAX_ERROR_TEXT = 500

/** Every non-2xx answer. Screens branch on `code`, never on `message`. */
export class ApiError extends Error {
  readonly status: number
  readonly body: ApiErrorBody
  readonly response: Response

  constructor(status: number, body: ApiErrorBody, response: Response) {
    super(body.message)
    this.name = 'ApiError'
    this.status = status
    this.body = body
    this.response = response
  }

  get code(): ErrorCode {
    return this.body.error_code
  }
}

/** The request never got an answer: offline, DNS, refused connection. */
export class NetworkError extends Error {
  constructor(cause: unknown) {
    super('Network request failed', { cause })
    this.name = 'NetworkError'
  }
}

type FetchLike = (input: string, init: RequestInit) => Promise<Response>

const isErrorCode = (value: unknown): value is ErrorCode =>
  errorCodes.some((code) => code === value)

/** A code for answers that are not ours, such as a proxy page. */
function codeForStatus(status: number): ErrorCode {
  if (status === 401) return 'unauthenticated'
  if (status === 403) return 'forbidden'
  if (status === 404) return 'not_found'
  if (status === 413) return 'payload_too_large'
  if (status === 429) return 'rate_limited'
  return 'internal'
}

async function errorBody(response: Response): Promise<ApiErrorBody> {
  const text = await response.text().catch(() => '')
  try {
    const parsed: unknown = JSON.parse(text)
    if (typeof parsed === 'object' && parsed !== null && 'error_code' in parsed) {
      const { error_code, message, errors, current } = parsed as Partial<ApiErrorBody>
      if (isErrorCode(error_code)) {
        return { error_code, message: message ?? '', errors, current }
      }
    }
  } catch {
    // Not JSON: fall through to the status-based body.
  }
  return {
    error_code: codeForStatus(response.status),
    message: text.slice(0, MAX_ERROR_TEXT) || response.statusText || `HTTP ${response.status}`,
  }
}

/** Query arguments are query-string values: arrays repeat the key, absent values are left out. */
function queryString(args: object): string {
  const params = new URLSearchParams()
  for (const [key, value] of Object.entries(args)) {
    if (value === undefined || value === null) continue
    for (const item of Array.isArray(value) ? value : [value]) params.append(key, String(item))
  }
  const serialized = params.toString()
  return serialized ? `?${serialized}` : ''
}

/**
 * Splits a server-sent event stream into the `data` of each event. Lines of one event are joined
 * with newlines; comments, `event`, `id` and `retry` fields are ignored. Returns the unfinished
 * tail for the next chunk.
 */
export function parseEvents(buffer: string, onData: (data: string) => void): string {
  const blocks = buffer.split(/\r?\n\r?\n/)
  const rest = blocks.pop() ?? ''
  for (const block of blocks) {
    const data = block
      .split(/\r?\n/)
      .filter((line) => line.startsWith('data:'))
      .map((line) => line.slice(line.startsWith('data: ') ? 6 : 5))
    if (data.length > 0) onData(data.join('\n'))
  }
  return rest
}

/**
 * The one transport. Resources call it; pages and components never call `fetch`. Types come
 * from the generated operation maps, so a renamed operation or changed argument is a compile
 * error here and in every caller.
 */
export class ApiClient {
  private authHandler: ((operation: string) => void) | null = null
  private enrollmentHandler: (() => void) | null = null
  private readonly observers: Array<(response: Response) => void> = []

  constructor(
    private readonly baseUrl: () => string,
    private readonly send: FetchLike = (input, init) => fetch(input, init)
  ) {}

  /** Called with the operation name when a call answers `unauthenticated`. */
  setAuthHandler(handler: (operation: string) => void): void {
    this.authHandler = handler
  }

  /** Called when the session may only enroll a second factor (a 2FA requirement applies). */
  setEnrollmentHandler(handler: () => void): void {
    this.enrollmentHandler = handler
  }

  /** Sees every response before it is parsed. */
  observe(observer: (response: Response) => void): void {
    this.observers.push(observer)
  }

  /** Runs a command. Reusing `idempotencyKey` repeats the same intent and returns the stored answer. */
  command<N extends CommandName>(
    name: N,
    args: CommandArgs<N>,
    options: { idempotencyKey?: string; signal?: AbortSignal } = {}
  ): Promise<CommandResult<N>> {
    const body = JSON.stringify({ idempotency_key: options.idempotencyKey ?? crypto.randomUUID(), ...args })
    return this.request(name, `${this.baseUrl()}/commands/${name}`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body,
      signal: options.signal,
    })
  }

  query<N extends QueryName>(
    name: N,
    args: QueryArgs<N>,
    options: { signal?: AbortSignal } = {}
  ): Promise<QuerySnapshot<N>> {
    return this.request(name, `${this.baseUrl()}/queries/${name}${queryString(args)}`, {
      method: 'GET',
      signal: options.signal,
    })
  }

  /**
   * Posts raw bytes, such as an image, to an upload route below the API base. The body is never
   * wrapped in JSON. Uploads answer like commands; `T` names the result the route documents.
   */
  upload<T extends object>(path: `/${string}`, body: Blob, options: { signal?: AbortSignal } = {}): Promise<CommandMeta & T> {
    return this.request(path, `${this.baseUrl()}${path}`, {
      method: 'POST',
      headers: { 'Content-Type': body.type || 'application/octet-stream' },
      body,
      signal: options.signal,
    })
  }

  /**
   * Reads `GET /api/subscribe` for up to 32 topics, each a topic (from its head) or a cursor (after
   * it), with `fetch`, so the stream carries the session cookie and the request marker like every
   * other call. Calls `onData` with each event's data and `onEnd` once
   * when the stream ends or fails, unless it was closed through the returned function.
   */
  subscribe(topics: readonly string[], onData: (data: string) => void, onEnd: () => void): () => void {
    const controller = new AbortController()
    const read = async () => {
      const response = await this.send(`${this.baseUrl()}/subscribe${queryString({ topic: topics })}`, {
        method: 'GET',
        signal: controller.signal,
        ...this.transport({ Accept: 'text/event-stream' }),
      })
      if (!response.ok || !response.body) return
      const reader = response.body.pipeThrough(new TextDecoderStream()).getReader()
      let buffer = ''
      for (;;) {
        const { done, value } = await reader.read()
        if (done) return
        buffer = parseEvents(buffer + value, onData)
      }
    }
    read()
      .catch(() => undefined)
      .finally(() => {
        if (!controller.signal.aborted) onEnd()
      })
    return () => controller.abort()
  }

  /** Credentials and the headers every call carries. */
  private transport(headers: HeadersInit = {}): Pick<RequestInit, 'credentials' | 'headers'> {
    return {
      credentials: 'same-origin',
      headers: { [REQUEST_MARKER_HEADER]: REQUEST_MARKER_VALUE, ...headers },
    }
  }

  private async request<T>(operation: string, url: string, init: RequestInit): Promise<T> {
    let response: Response
    try {
      response = await this.send(url, { ...init, ...this.transport({ Accept: 'application/json', ...init.headers }) })
    } catch (error) {
      // An abort is the caller's own decision, not a network failure.
      if (error instanceof DOMException && error.name === 'AbortError') throw error
      throw new NetworkError(error)
    }

    for (const observer of this.observers) observer(response)

    if (!response.ok) {
      const error = new ApiError(response.status, await errorBody(response), response)
      if (error.code === 'unauthenticated') this.authHandler?.(operation)
      if (error.code === 'second_factor_enrollment_required') this.enrollmentHandler?.()
      throw error
    }
    if (response.status === 204) return {} as T
    // The server is the type's source: answers are generated from the same registry.
    return (await response.json()) as T
  }
}
