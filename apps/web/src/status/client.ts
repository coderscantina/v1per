import type {
  ApiErrorBody,
  ErrorCode,
  LiveFrame,
  Locale,
  PageEmailBody,
  PageIncidents,
  PagePasswordBody,
  PageRedeemBody,
  PageSnapshot,
} from '~/generated'

/**
 * The status page's one network module: the page routes below its base
 * (`docs/contracts/status-page-api.md`). Components call these functions, never `fetch`.
 * The page cookie rides along on its own, so no call carries a token.
 */

/** A failed call: the server's error code, or `network` when it was not reached. */
export class PageError extends Error {
  constructor(readonly code: ErrorCode | 'network') {
    super(code)
  }
}

async function call(url: string, init: RequestInit = {}): Promise<Response> {
  let response: Response
  try {
    response = await fetch(url, { credentials: 'same-origin', ...init })
  } catch {
    throw new PageError('network')
  }
  if (response.ok) return response
  const body = (await response.json().catch(() => null)) as Partial<ApiErrorBody> | null
  throw new PageError(body?.error_code ?? 'internal')
}

function post(url: string, body: PagePasswordBody | PageEmailBody | PageRedeemBody): Promise<Response> {
  return call(url, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json', Accept: 'application/json' },
    body: JSON.stringify(body),
  })
}

/** `?locale=` of the address, passed on so a chosen language sticks across reads. */
function localeQuery(): string {
  const asked = new URLSearchParams(window.location.search).get('locale')
  return asked ? `?locale=${encodeURIComponent(asked)}` : ''
}

export function pageClient(base: string) {
  return {
    async snapshot(): Promise<PageSnapshot> {
      const response = await call(`${base}/snapshot.json${localeQuery()}`, {
        headers: { Accept: 'application/json' },
        cache: 'no-store',
      })
      return (await response.json()) as PageSnapshot
    },

    /** One page of the incident history, newest first; `cursor` is the previous page's `next_cursor`. */
    async incidents(limit: number, cursor?: string): Promise<PageIncidents> {
      const query = new URLSearchParams({ limit: String(limit) })
      if (cursor) query.set('cursor', cursor)
      const asked = new URLSearchParams(window.location.search).get('locale')
      if (asked) query.set('locale', asked)
      const response = await call(`${base}/incidents.json?${query}`, {
        headers: { Accept: 'application/json' },
        cache: 'no-store',
      })
      return (await response.json()) as PageIncidents
    },

    async password(password: string): Promise<void> {
      await post(`${base}/access/password`, { password })
    },

    async requestLink(email: string, locale: Locale): Promise<void> {
      await post(`${base}/access/email`, { email, locale })
    },

    async redeem(code: string): Promise<void> {
      await post(`${base}/access/email/redeem`, { code })
    },

    /**
     * Opens `/events` from `cursor`. Calls `onFrame` per frame and `onError` once when the
     * stream fails or ends; the caller decides when to reconnect. Returns the close function.
     */
    events(cursor: string, onFrame: (frame: LiveFrame) => void, onError: () => void): () => void {
      const source = new EventSource(`${base}/events?cursor=${encodeURIComponent(cursor)}`)
      let open = true
      const close = () => {
        open = false
        source.close()
      }
      source.onmessage = (message: MessageEvent<string>) => {
        let frame: LiveFrame
        try {
          frame = JSON.parse(message.data) as LiveFrame
        } catch {
          return
        }
        onFrame(frame)
      }
      // EventSource would retry with the old cursor; close and let the caller resume.
      source.onerror = () => {
        if (!open) return
        close()
        onError()
      }
      return close
    },
  }
}

export type PageClient = ReturnType<typeof pageClient>
