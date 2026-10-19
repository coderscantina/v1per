import { describe, expect, it, vi } from 'vitest'

import { ApiClient, ApiError, NetworkError, parseEvents } from './client'

interface Sent {
  url: string
  init: RequestInit
}

/** A client whose transport records requests and answers with `response`. */
function clientAnswering(response: () => Response) {
  const sent: Sent[] = []
  const client = new ApiClient(
    () => '/api',
    async (url, init) => {
      sent.push({ url, init })
      return response()
    }
  )
  return { client, sent }
}

const json = (body: unknown, status = 200) =>
  new Response(JSON.stringify(body), { status, headers: { 'Content-Type': 'application/json' } })

const header = (init: RequestInit, name: string) => new Headers(init.headers).get(name)

describe('ApiClient', () => {
  it('sends commands same-origin with the request marker and an idempotency key', async () => {
    const { client, sent } = clientAnswering(() => json({ revision: 3, status: 'signed_in', account: {} }))

    await client.command('auth.login', { email: 'a@example.test', password: 'pw' })
    await client.command('auth.login', { email: 'a@example.test', password: 'pw' }, { idempotencyKey: 'k1' })

    const [generated, given] = sent.map((request) => JSON.parse(String(request.init.body)))
    expect(sent[0]?.url).toBe('/api/commands/auth.login')
    expect(sent[0]?.init.method).toBe('POST')
    expect(sent[0]?.init.credentials).toBe('same-origin')
    expect(header(sent[0]!.init, 'x-v1per-request')).toBe('1')
    expect(generated.idempotency_key).toMatch(/^[0-9a-f-]{36}$/)
    expect(given).toEqual({ idempotency_key: 'k1', email: 'a@example.test', password: 'pw' })
  })

  it('serializes query arguments into the query string and passes the abort signal', async () => {
    const { client, sent } = clientAnswering(() => json({ revision: 1, cursor: 'instance@1', account: null }))
    const controller = new AbortController()

    await client.query('auth.session', {}, { signal: controller.signal })

    expect(sent[0]?.url).toBe('/api/queries/auth.session')
    expect(sent[0]?.init.method).toBe('GET')
    expect(sent[0]?.init.signal).toBe(controller.signal)
    expect(header(sent[0]!.init, 'x-v1per-request')).toBe('1')
  })

  it('uploads raw bytes with the request marker, never inside JSON', async () => {
    const { client, sent } = clientAnswering(() => json({ revision: 4, page: { id: '1' } }))
    const image = new Blob([new Uint8Array([0x89, 0x50, 0x4e, 0x47])], { type: 'image/png' })

    const answer = await client.upload<{ page: { id: string } }>('/pages/1/logo', image)

    expect(answer).toEqual({ revision: 4, page: { id: '1' } })
    expect(sent[0]?.url).toBe('/api/pages/1/logo')
    expect(sent[0]?.init.method).toBe('POST')
    expect(sent[0]?.init.body).toBe(image)
    expect(header(sent[0]!.init, 'content-type')).toBe('image/png')
    expect(header(sent[0]!.init, 'x-v1per-request')).toBe('1')
  })

  it('throws one error type carrying the typed body', async () => {
    const body = { error_code: 'invalid_arguments', message: 'x', errors: { email: [{ code: 'required' }] } }
    const { client } = clientAnswering(() => json(body, 422))

    const error = await client.command('auth.login', { email: '', password: '' }).catch((e: unknown) => e)

    expect(error).toBeInstanceOf(ApiError)
    expect(error).toMatchObject({ status: 422, code: 'invalid_arguments', body })
  })

  it('cuts non-JSON error bodies to 500 characters and falls back to the HTTP status', async () => {
    const page = `<html>${'x'.repeat(2000)}</html>`
    const { client } = clientAnswering(() => new Response(page, { status: 502, statusText: '' }))
    const empty = clientAnswering(() => new Response('', { status: 503, statusText: '' }))

    const error = await client.query('auth.session', {}).catch((e: unknown) => e)
    const bare = await empty.client.query('auth.session', {}).catch((e: unknown) => e)

    expect(error).toMatchObject({ status: 502, code: 'internal' })
    expect((error as ApiError).message).toHaveLength(500)
    expect((bare as ApiError).message).toBe('HTTP 503')
  })

  it('calls the auth handler only for unauthenticated answers', async () => {
    const signedOut = clientAnswering(() => json({ error_code: 'unauthenticated', message: '' }, 401))
    const wrongPassword = clientAnswering(() => json({ error_code: 'invalid_credentials', message: '' }, 401))
    const handler = vi.fn()
    signedOut.client.setAuthHandler(handler)
    wrongPassword.client.setAuthHandler(handler)

    await wrongPassword.client.command('auth.login', { email: 'a', password: 'b' }).catch(() => {})
    expect(handler).not.toHaveBeenCalled()

    await signedOut.client.query('instance.info', {}).catch(() => {})
    expect(handler).toHaveBeenCalledWith('instance.info')
  })

  it('tells a lost connection apart from an aborted request', async () => {
    const offline = new ApiClient(
      () => '/api',
      async () => {
        throw new TypeError('Failed to fetch')
      }
    )
    const aborted = new ApiClient(
      () => '/api',
      async () => {
        throw new DOMException('aborted', 'AbortError')
      }
    )

    await expect(offline.query('auth.session', {})).rejects.toBeInstanceOf(NetworkError)
    await expect(aborted.query('auth.session', {})).rejects.toMatchObject({ name: 'AbortError' })
  })

  it('splits server-sent events across chunks and joins multi-line data', () => {
    const data: string[] = []
    const rest = parseEvents(': ping\n\ndata: {"a":1}\n\nevent: x\ndata: one\ndata: two\n\ndata: {"b"', (d) => data.push(d))
    expect(data).toEqual(['{"a":1}', 'one\ntwo'])
    expect(parseEvents(`${rest}:2}\n\n`, (d) => data.push(d))).toBe('')
    expect(data.at(-1)).toBe('{"b":2}')
  })

  it('streams a subscription with the request marker and reports the end once', async () => {
    const encoder = new TextEncoder()
    const body = new ReadableStream<Uint8Array>({
      start(controller) {
        controller.enqueue(encoder.encode('data: {"kind":"subscribed"'))
        controller.enqueue(encoder.encode(',"head":"h","cursor":"c"}\n\n'))
        controller.close()
      },
    })
    const { client, sent } = clientAnswering(() => new Response(body, { status: 200 }))
    const frames: string[] = []
    const ended = new Promise<void>((resolve) => {
      client.subscribe(['user:u1@3', 'instance'], (d) => frames.push(d), resolve)
    })

    await ended

    expect(sent[0]?.url).toBe('/api/subscribe?topic=user%3Au1%403&topic=instance')
    expect(sent[0]?.init.credentials).toBe('same-origin')
    expect(header(sent[0]!.init, 'x-v1per-request')).toBe('1')
    expect(frames).toEqual(['{"kind":"subscribed","head":"h","cursor":"c"}'])
  })
})
