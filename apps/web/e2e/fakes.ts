import { createServer as createHttpServer, type Server } from 'node:http'
import { createServer as createTcpServer, type AddressInfo, type Server as TcpServer } from 'node:net'

/**
 * Local stand-ins for the services alerts go to, like the server's own test fakes
 * (`apps/server/tests/common/fakes.rs`): an SMTP server that keeps every message, a Discord
 * webhook that records every post, and a website that answers 200 until it is stopped.
 */

export interface Mail {
  from: string
  to: string[]
  data: string
}

/** The value of a header in a raw message, unfolded. */
export function header(mail: Mail, name: string): string | undefined {
  const head = mail.data.split(/\r?\n\r?\n/)[0] ?? ''
  const unfolded = head.replace(/\r?\n[ \t]+/g, ' ')
  const line = unfolded.split(/\r?\n/).find((entry) => entry.toLowerCase().startsWith(`${name.toLowerCase()}:`))
  return line?.slice(name.length + 1).trim()
}

/**
 * Decodes RFC 2047 encoded words (`=?UTF-8?B?...?=` and `=?UTF-8?Q?...?=`), as subjects with emoji
 * use. Whitespace between two encoded words is dropped, as the RFC says.
 */
export function decodeWords(text: string): string {
  return text.replace(/=\?([^?]+)\?([BbQq])\?([^?]*)\?=(?:\s+(?==\?))?/g, (_, _charset: string, encoding: string, value: string) => {
    if (encoding.toUpperCase() === 'B') return Buffer.from(value, 'base64').toString('utf8')
    const bytes = value.replace(/_/g, ' ').replace(/=([0-9A-Fa-f]{2})/g, (_match, hex: string) => String.fromCharCode(Number.parseInt(hex, 16)))
    return Buffer.from(bytes, 'latin1').toString('utf8')
  })
}

/** An SMTP server without TLS that accepts any login and keeps every message. */
export async function startSmtpSink(): Promise<{ port: number; mails: Mail[]; stop: () => Promise<void> }> {
  const mails: Mail[] = []
  const server: TcpServer = createTcpServer((socket) => {
    let buffer = ''
    let mail: Mail = { from: '', to: [], data: '' }
    let data: string[] | null = null
    const reply = (line: string) => socket.write(`${line}\r\n`)
    reply('220 sink ESMTP')
    socket.on('data', (chunk) => {
      buffer += chunk.toString('utf8')
      let at = buffer.indexOf('\r\n')
      while (at >= 0) {
        const line = buffer.slice(0, at)
        buffer = buffer.slice(at + 2)
        at = buffer.indexOf('\r\n')
        if (data) {
          if (line === '.') {
            mail.data = data.join('\r\n')
            mails.push(mail)
            mail = { from: '', to: [], data: '' }
            data = null
            reply('250 queued')
          } else {
            data.push(line.startsWith('.') ? line.slice(1) : line)
          }
          continue
        }
        const upper = line.toUpperCase()
        if (upper.startsWith('EHLO') || upper.startsWith('HELO')) socket.write('250-sink\r\n250 AUTH PLAIN\r\n')
        else if (upper.startsWith('AUTH')) reply('235 accepted')
        else if (upper.startsWith('MAIL FROM:')) {
          mail.from = line.slice(10).trim()
          reply('250 OK')
        } else if (upper.startsWith('RCPT TO:')) {
          mail.to.push(line.slice(8).trim())
          reply('250 OK')
        } else if (upper === 'DATA') {
          data = []
          reply('354 go ahead')
        } else if (upper === 'QUIT') {
          reply('221 bye')
          socket.end()
        } else reply('250 OK')
      }
    })
    socket.on('error', () => undefined)
  })
  await new Promise<void>((done) => server.listen(0, '127.0.0.1', done))
  return {
    port: (server.address() as AddressInfo).port,
    mails,
    stop: () => new Promise((done) => server.close(() => done())),
  }
}

/** A Discord webhook: answers the avatar check and records every JSON post. */
export async function startDiscordStub(): Promise<{ url: string; posts: unknown[]; stop: () => Promise<void> }> {
  const posts: unknown[] = []
  const server: Server = createHttpServer((request, response) => {
    if (request.method === 'GET') {
      response.writeHead(200, { 'content-type': 'application/json' })
      response.end(JSON.stringify({ avatar: 'stub' }))
      return
    }
    let body = ''
    request.on('data', (chunk: Buffer) => (body += chunk.toString('utf8')))
    request.on('end', () => {
      posts.push(JSON.parse(body))
      response.writeHead(204)
      response.end()
    })
  })
  await new Promise<void>((done) => server.listen(0, '127.0.0.1', done))
  return {
    url: `http://127.0.0.1:${(server.address() as AddressInfo).port}/api/webhooks/1/stub-token`,
    posts,
    stop: async () => {
      server.closeAllConnections()
      await new Promise((done) => server.close(done))
    },
  }
}

/** A plain HTTP endpoint that answers 200, standing in for a website. */
export async function startTarget(): Promise<{ url: string; stop: () => Promise<void> }> {
  const server = createHttpServer((_, response) => {
    response.writeHead(200, { 'content-type': 'text/plain' })
    response.end('ok')
  })
  await new Promise<void>((done) => server.listen(0, '127.0.0.1', done))
  let stopped = false
  return {
    url: `http://127.0.0.1:${(server.address() as AddressInfo).port}/health`,
    stop: async () => {
      if (stopped) return
      stopped = true
      server.closeAllConnections()
      await new Promise((done) => server.close(done))
    },
  }
}
