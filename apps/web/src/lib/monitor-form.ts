import type {
  ChannelChoice,
  ChannelId,
  DnsRecordType,
  FieldErrorCode,
  HttpAuth,
  HttpBodyEncoding,
  HttpHeader,
  HttpSettings,
  IpFamily,
  JsonPathOperator,
  Monitor,
  MonitorId,
  MonitorSettings,
  MonitorsCreateArgs,
  MonitorType,
  OAuthClientAuth,
  ProbeChoice,
  ProbeId,
  Quorum,
  SecretField,
} from '~/generated'
import { MONITOR_TAGS_MAX, PUSH_PATH, PUSH_TOKEN_MAX_CHARS, PUSH_TOKEN_MIN_CHARS } from '~/generated/limits'

/**
 * The monitor editor's state, one shape for every type: switching the type keeps what was typed
 * for the others, and only the chosen type's fields are sent. Numbers stay text while editing, so
 * an empty or half-typed field is not silently a 0. Field errors are keyed by the argument path
 * the server uses (`settings.url`), so client and server errors land on the same field.
 */

/** Types a person can create. `unsupported` only comes from a Kuma import. */
export const creatableTypes = ['http', 'keyword', 'json-query', 'port', 'ping', 'dns', 'push', 'group', 'manual'] as const
export type CreatableType = (typeof creatableTypes)[number]

export const isCreatable = (type: MonitorType): type is CreatableType => creatableTypes.some((entry) => entry === type)

/** Push, group and manual monitors run on the server; the rest run on probes. */
export const runsOnProbes = (type: CreatableType) => !['push', 'group', 'manual'].includes(type)

/** Types that send an HTTP request and share its options. */
export const requestTypes = ['http', 'keyword', 'json-query'] as const satisfies readonly CreatableType[]
export const sendsRequest = (type: CreatableType) => requestTypes.some((entry) => entry === type)

export const httpMethods = ['GET', 'POST', 'PUT', 'PATCH', 'DELETE', 'HEAD', 'OPTIONS'] as const
export const bodyEncodings = ['json', 'form', 'xml'] as const satisfies readonly HttpBodyEncoding[]
export const authMethods = ['none', 'basic', 'bearer', 'oauth2-cc'] as const satisfies readonly HttpAuth['auth_method'][]
export const oauthClientAuths = ['client_secret_basic', 'client_secret_post'] as const satisfies readonly OAuthClientAuth[]
export const jsonOperators = ['==', '!=', '<', '<=', '>', '>=', 'contains'] as const satisfies readonly JsonPathOperator[]
export const dnsRecordTypes = ['A', 'AAAA', 'CNAME', 'MX', 'NS', 'TXT', 'SRV', 'PTR', 'CAA', 'SOA'] as const satisfies readonly DnsRecordType[]
export const quorums = ['majority', 'any', 'all'] as const satisfies readonly Quorum[]

/**
 * A sealed credential in the editor. A stored one is kept unless the person replaces or clears
 * it; the stored value is never shown.
 */
export interface SecretInput {
  /** A value is stored on the server. */
  set: boolean
  mode: 'keep' | 'replace' | 'clear'
  value: string
}

export const newSecret = (): SecretInput => ({ set: false, mode: 'replace', value: '' })
const storedSecret = (field: SecretField): SecretInput => ({ set: field.set, mode: field.set ? 'keep' : 'replace', value: '' })

/** Keep sends neither value nor clear; clear removes the stored value, so the check sends none. */
export function secretField(input: SecretInput): SecretField {
  if (input.set && input.mode === 'keep') return { set: true }
  if (input.mode === 'clear') return { clear: true, set: false }
  return { value: input.value, set: false }
}

export interface HeaderInput {
  name: string
  value: string
  /** A sealed header, such as an API key; `value` is unused then. */
  secret: SecretInput | null
}

export interface RequestInput {
  url: string
  method: string
  headers: HeaderInput[]
  body: string
  bodyEncoding: HttpBodyEncoding
  authMethod: HttpAuth['auth_method']
  basicUser: string
  basicPass: SecretInput
  bearerToken: SecretInput
  oauthTokenUrl: string
  oauthClientId: string
  oauthClientSecret: SecretInput
  oauthScopes: string
  oauthAudience: string
  oauthClientAuth: OAuthClientAuth
  /** Comma separated codes or ranges, such as `200-299, 304`. */
  acceptedStatusCodes: string
  maxRedirects: string
  ignoreTls: boolean
  ipFamily: IpFamily | 'any'
  cacheBust: boolean
  /** Alert when the certificate is about to expire (Kuma's `expiryNotification`). */
  expiryNotification: boolean
}

export interface MonitorForm {
  type: CreatableType
  name: string
  description: string
  /** Seconds. */
  interval: string
  /** Seconds; empty takes the interval. */
  retryInterval: string
  maxRetries: string
  resendInterval: string
  /** Seconds; empty takes 80 % of the interval. */
  timeout: string
  upsideDown: boolean
  quorum: Quorum
  /** Comma separated. */
  tags: string
  group: MonitorId | null
  request: RequestInput
  keyword: string
  invertKeyword: boolean
  jsonPath: string
  jsonOperator: JsonPathOperator
  expectedValue: string
  retryOnlyOnStatusCodeFailure: boolean
  hostname: string
  port: string
  pingCount: string
  packetSize: string
  pingTimeout: string
  dnsRecordType: DnsRecordType
  dnsServer: string
  dnsPort: string
  /** A new push token to set; `null` keeps the stored one, or lets the server make one. */
  pushToken: string | null
  /** The probes that run the check, kept as stored; empty means the built-in probe. */
  probes: ProbeId[]
  /**
   * The channels it alerts; `null` until picked, which on create means the default channels of
   * the node and its ancestors, as the server applies them.
   */
  channels: ChannelId[] | null
}

/** Kuma's defaults for a new monitor. */
export function emptyForm(type: CreatableType = 'http'): MonitorForm {
  return {
    type,
    name: '',
    description: '',
    interval: '60',
    retryInterval: '',
    maxRetries: '0',
    resendInterval: '0',
    timeout: '',
    upsideDown: false,
    quorum: 'majority',
    tags: '',
    group: null,
    request: {
      url: 'https://',
      method: 'GET',
      headers: [],
      body: '',
      bodyEncoding: 'json',
      authMethod: 'none',
      basicUser: '',
      basicPass: newSecret(),
      bearerToken: newSecret(),
      oauthTokenUrl: '',
      oauthClientId: '',
      oauthClientSecret: newSecret(),
      oauthScopes: '',
      oauthAudience: '',
      oauthClientAuth: 'client_secret_basic',
      acceptedStatusCodes: '200-299',
      maxRedirects: '10',
      ignoreTls: false,
      ipFamily: 'any',
      cacheBust: false,
      expiryNotification: false,
    },
    keyword: '',
    invertKeyword: false,
    jsonPath: '',
    jsonOperator: '==',
    expectedValue: '',
    retryOnlyOnStatusCodeFailure: false,
    hostname: '',
    port: '',
    pingCount: '1',
    packetSize: '56',
    pingTimeout: '2',
    dnsRecordType: 'A',
    dnsServer: '1.1.1.1',
    dnsPort: '53',
    pushToken: null,
    probes: [],
    channels: null,
  }
}

function requestInput(settings: HttpSettings, base: RequestInput): RequestInput {
  const { auth } = settings
  return {
    ...base,
    url: settings.url,
    method: settings.method,
    headers: settings.headers.map((header) => ({
      name: header.name,
      value: header.value ?? '',
      secret: header.secret ? storedSecret(header.secret) : null,
    })),
    body: settings.body ?? '',
    bodyEncoding: settings.http_body_encoding,
    authMethod: auth.auth_method,
    basicUser: auth.auth_method === 'basic' ? auth.basic_auth_user : '',
    basicPass: auth.auth_method === 'basic' ? storedSecret(auth.basic_auth_pass) : newSecret(),
    bearerToken: auth.auth_method === 'bearer' ? storedSecret(auth.bearer_token) : newSecret(),
    oauthTokenUrl: auth.auth_method === 'oauth2-cc' ? auth.oauth_token_url : '',
    oauthClientId: auth.auth_method === 'oauth2-cc' ? auth.oauth_client_id : '',
    oauthClientSecret: auth.auth_method === 'oauth2-cc' ? storedSecret(auth.oauth_client_secret) : newSecret(),
    oauthScopes: auth.auth_method === 'oauth2-cc' ? (auth.oauth_scopes ?? '') : '',
    oauthAudience: auth.auth_method === 'oauth2-cc' ? (auth.oauth_audience ?? '') : '',
    oauthClientAuth: auth.auth_method === 'oauth2-cc' ? auth.oauth_auth_method : base.oauthClientAuth,
    acceptedStatusCodes: settings.accepted_statuscodes.join(', '),
    maxRedirects: String(settings.maxredirects),
    ignoreTls: settings.ignore_tls,
    ipFamily: settings.ip_family ?? 'any',
    cacheBust: settings.cache_bust,
    expiryNotification: settings.expiry_notification ?? false,
  }
}

/** The editor state of a stored monitor; `null` for an imported type V1Per does not run. */
export function formFromMonitor(monitor: Monitor): MonitorForm | null {
  const { settings } = monitor
  if (settings.type === 'unsupported') return null
  const form: MonitorForm = {
    ...emptyForm(settings.type),
    name: monitor.name,
    description: monitor.description,
    interval: String(monitor.interval),
    retryInterval: String(monitor.retry_interval),
    maxRetries: String(monitor.max_retries),
    resendInterval: String(monitor.resend_interval),
    timeout: String(monitor.timeout),
    upsideDown: monitor.upside_down,
    quorum: monitor.quorum,
    tags: monitor.tags.join(', '),
    group: monitor.group ?? null,
    probes: monitor.probes,
    channels: monitor.channels,
  }
  switch (settings.type) {
    case 'http':
      return { ...form, request: requestInput(settings, form.request) }
    case 'keyword':
      return {
        ...form,
        request: requestInput(settings, form.request),
        keyword: settings.keyword,
        invertKeyword: settings.invert_keyword,
      }
    case 'json-query':
      return {
        ...form,
        request: requestInput(settings, form.request),
        jsonPath: settings.json_path,
        jsonOperator: settings.json_path_operator,
        expectedValue: settings.expected_value,
        retryOnlyOnStatusCodeFailure: settings.retry_only_on_status_code_failure,
      }
    case 'port':
      return { ...form, hostname: settings.hostname, port: String(settings.port) }
    case 'ping':
      return {
        ...form,
        hostname: settings.hostname,
        pingCount: String(settings.ping_count),
        packetSize: String(settings.packet_size),
        pingTimeout: String(settings.ping_per_request_timeout),
      }
    case 'dns':
      return {
        ...form,
        hostname: settings.hostname,
        dnsRecordType: settings.dns_resolve_type,
        dnsServer: settings.dns_resolve_server,
        dnsPort: String(settings.port),
      }
    case 'push':
    case 'group':
    case 'manual':
      return form
  }
}

const listOf = (text: string) =>
  text
    .split(/[,\n]/)
    .map((entry) => entry.trim())
    .filter((entry) => entry.length > 0)

/** Text as a number; `NaN` when it is not one, so validation and the server both refuse it. */
const numberOf = (text: string) => (text.trim() === '' ? Number.NaN : Number(text))
const optionalNumber = (text: string) => (text.trim() === '' ? undefined : Number(text))

function httpAuth(request: RequestInput): HttpAuth {
  switch (request.authMethod) {
    case 'none':
      return { auth_method: 'none' }
    case 'basic':
      return { auth_method: 'basic', basic_auth_user: request.basicUser, basic_auth_pass: secretField(request.basicPass) }
    case 'bearer':
      return { auth_method: 'bearer', bearer_token: secretField(request.bearerToken) }
    case 'oauth2-cc':
      return {
        auth_method: 'oauth2-cc',
        oauth_token_url: request.oauthTokenUrl.trim(),
        oauth_client_id: request.oauthClientId.trim(),
        oauth_client_secret: secretField(request.oauthClientSecret),
        oauth_scopes: request.oauthScopes.trim() || undefined,
        oauth_audience: request.oauthAudience.trim() || undefined,
        oauth_auth_method: request.oauthClientAuth,
      }
  }
}

/** Rows without a name are left out, so an empty row added by mistake is not an error. */
function httpHeaders(headers: HeaderInput[]): HttpHeader[] {
  return headers
    .filter((header) => header.name.trim() !== '')
    .map((header) =>
      header.secret ? { name: header.name.trim(), secret: secretField(header.secret) } : { name: header.name.trim(), value: header.value }
    )
}

function httpSettings(request: RequestInput): HttpSettings {
  return {
    url: request.url.trim(),
    method: request.method,
    headers: httpHeaders(request.headers),
    body: request.body === '' ? undefined : request.body,
    http_body_encoding: request.bodyEncoding,
    auth: httpAuth(request),
    accepted_statuscodes: listOf(request.acceptedStatusCodes),
    maxredirects: numberOf(request.maxRedirects),
    ignore_tls: request.ignoreTls,
    ip_family: request.ipFamily === 'any' ? undefined : request.ipFamily,
    cache_bust: request.cacheBust,
    expiry_notification: request.expiryNotification,
  }
}

/** The chosen type's settings, without the fields of the others. */
export function toSettings(form: MonitorForm): MonitorSettings {
  switch (form.type) {
    case 'http':
      return { type: 'http', ...httpSettings(form.request) }
    case 'keyword':
      return { type: 'keyword', ...httpSettings(form.request), keyword: form.keyword, invert_keyword: form.invertKeyword }
    case 'json-query':
      return {
        type: 'json-query',
        ...httpSettings(form.request),
        json_path: form.jsonPath.trim(),
        json_path_operator: form.jsonOperator,
        expected_value: form.expectedValue,
        retry_only_on_status_code_failure: form.retryOnlyOnStatusCodeFailure,
      }
    case 'port':
      return { type: 'port', hostname: form.hostname.trim(), port: numberOf(form.port) }
    case 'ping':
      return {
        type: 'ping',
        hostname: form.hostname.trim(),
        ping_count: numberOf(form.pingCount),
        packet_size: numberOf(form.packetSize),
        ping_per_request_timeout: numberOf(form.pingTimeout),
      }
    case 'dns':
      return {
        type: 'dns',
        hostname: form.hostname.trim(),
        dns_resolve_type: form.dnsRecordType,
        dns_resolve_server: form.dnsServer.trim(),
        port: numberOf(form.dnsPort),
      }
    case 'push':
      return form.pushToken === null ? { type: 'push' } : { type: 'push', push_token: form.pushToken }
    case 'group':
      return { type: 'group' }
    case 'manual':
      return { type: 'manual' }
  }
}

/** What `monitors.create` (with the node) and `monitors.update` (with the monitor) both take. */
export type MonitorInput = Omit<MonitorsCreateArgs, 'node'>

export function toInput(form: MonitorForm): MonitorInput {
  return {
    name: form.name.trim(),
    description: form.description.trim(),
    settings: toSettings(form),
    interval: numberOf(form.interval),
    retry_interval: optionalNumber(form.retryInterval),
    max_retries: numberOf(form.maxRetries),
    resend_interval: numberOf(form.resendInterval),
    timeout: optionalNumber(form.timeout),
    upside_down: form.upsideDown,
    quorum: form.quorum,
    tags: listOf(form.tags),
    group: form.group ?? undefined,
    probes: runsOnProbes(form.type) ? form.probes : [],
    channels: form.channels ?? undefined,
  }
}

/** The channels the picker shows as chosen: the picked ones, or the defaults until a pick. */
export const chosenChannels = (picked: readonly ChannelId[] | null, choices: readonly ChannelChoice[]): ChannelId[] =>
  picked ? [...picked] : choices.filter((choice) => choice.is_default).map((choice) => choice.id)

/**
 * The probes the picker shows as chosen: the stored ones, or the built-in probe while none are,
 * since an empty list means the built-in probe.
 */
export function chosenProbes(stored: readonly ProbeId[], choices: readonly ProbeChoice[]): ProbeId[] {
  if (stored.length > 0) return [...stored]
  const builtin = choices.find((choice) => choice.builtin)
  return builtin ? [builtin.id] : []
}

/**
 * What the form keeps after a pick: the probes on offer, in their order. Picking none leaves the
 * list empty, which is the built-in probe again.
 */
export const pickProbes = (picked: readonly ProbeId[], choices: readonly ProbeChoice[]): ProbeId[] =>
  choices.filter((choice) => picked.includes(choice.id)).map((choice) => choice.id)

/** The quorum only decides anything for a checked type on more than one probe. */
export const quorumApplies = (form: Pick<MonitorForm, 'type' | 'probes'>, choices: readonly ProbeChoice[]): boolean =>
  runsOnProbes(form.type) && chosenProbes(form.probes, choices).length > 1

export type FormErrors = Partial<Record<string, { code: FieldErrorCode; limit?: number }>>

const isWhole = (text: string) => /^\d+$/.test(text.trim())
const isDecimal = (text: string) => /^\d+(\.\d+)?$/.test(text.trim())

/**
 * What the browser can tell before sending: missing values, numbers that are not numbers and a
 * push token outside its length. Ranges and formats are the server's to judge; its field errors
 * land on the same keys.
 */
export function validate(form: MonitorForm): FormErrors {
  const errors: FormErrors = {}
  const required = (key: string, text: string) => {
    if (text.trim() === '') errors[key] = { code: 'required' }
  }
  const whole = (key: string, text: string, optional = false) => {
    if (text.trim() === '') {
      if (!optional) errors[key] = { code: 'required' }
    } else if (!isWhole(text)) errors[key] = { code: 'invalid_format' }
  }
  const decimal = (key: string, text: string, optional = false) => {
    if (text.trim() === '') {
      if (!optional) errors[key] = { code: 'required' }
    } else if (!isDecimal(text)) errors[key] = { code: 'invalid_format' }
  }

  required('name', form.name)
  whole('interval', form.interval)
  whole('retry_interval', form.retryInterval, true)
  whole('max_retries', form.maxRetries)
  whole('resend_interval', form.resendInterval)
  decimal('timeout', form.timeout, true)
  if (listOf(form.tags).length > MONITOR_TAGS_MAX) errors.tags = { code: 'too_many', limit: MONITOR_TAGS_MAX }

  if (sendsRequest(form.type)) {
    const { request } = form
    required('settings.url', request.url)
    whole('settings.maxredirects', request.maxRedirects)
    if (listOf(request.acceptedStatusCodes).length === 0) errors['settings.accepted_statuscodes'] = { code: 'required' }
    if (request.authMethod === 'oauth2-cc') {
      required('settings.auth.oauth_token_url', request.oauthTokenUrl)
      required('settings.auth.oauth_client_id', request.oauthClientId)
    }
  }
  switch (form.type) {
    case 'keyword':
      required('settings.keyword', form.keyword)
      break
    case 'port':
      required('settings.hostname', form.hostname)
      whole('settings.port', form.port)
      break
    case 'ping':
      required('settings.hostname', form.hostname)
      whole('settings.ping_count', form.pingCount)
      whole('settings.packet_size', form.packetSize)
      decimal('settings.ping_per_request_timeout', form.pingTimeout)
      break
    case 'dns':
      required('settings.hostname', form.hostname)
      required('settings.dns_resolve_server', form.dnsServer)
      whole('settings.port', form.dnsPort)
      break
    case 'push':
      if (form.pushToken === null) break
      if (form.pushToken.length < PUSH_TOKEN_MIN_CHARS) {
        errors['settings.push_token'] = { code: 'too_short', limit: PUSH_TOKEN_MIN_CHARS }
      } else if (form.pushToken.length > PUSH_TOKEN_MAX_CHARS) {
        errors['settings.push_token'] = { code: 'too_long', limit: PUSH_TOKEN_MAX_CHARS }
      }
      break
  }
  return errors
}

/** A random push token like Kuma's: 32 letters and digits. */
export function newPushToken(): string {
  const alphabet = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789'
  const bytes = crypto.getRandomValues(new Uint8Array(32))
  // The slight modulo bias leaves about 190 bits of entropy, plenty for a URL token.
  return Array.from(bytes, (byte) => alphabet[byte % alphabet.length]).join('')
}

/** The URL a push monitor's sender calls, with Kuma's example parameters. */
export const pushUrl = (origin: string, token: string): string =>
  `${origin.replace(/\/+$/, '')}${PUSH_PATH}${encodeURIComponent(token)}?status=up&msg=OK&ping=`
