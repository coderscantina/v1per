import { describe, expect, it } from 'vitest'

import type { ChannelChoice, HttpSettings, Monitor, MonitorSettings, ProbeChoice } from '~/generated'
import { PUSH_TOKEN_MAX_CHARS, PUSH_TOKEN_MIN_CHARS } from '~/generated/limits'

import {
  chosenChannels,
  chosenProbes,
  creatableTypes,
  emptyForm,
  formFromMonitor,
  newPushToken,
  newSecret,
  pickProbes,
  pushUrl,
  quorumApplies,
  secretField,
  toInput,
  validate,
  type MonitorForm,
} from './monitor-form'

const stored = (settings: MonitorSettings): Monitor => ({
  id: '7',
  node: '1',
  name: 'Shop',
  description: '',
  settings,
  interval: 60,
  retry_interval: 30,
  max_retries: 2,
  resend_interval: 0,
  timeout: 48,
  upside_down: false,
  quorum: 'majority',
  tags: ['prod', 'web'],
  probes: ['2'],
  channels: [],
  active: true,
  revision: 3,
  created_at: 0,
  updated_at: 0,
})

const request: HttpSettings = {
  url: 'https://shop.example.test/health',
  method: 'GET',
  headers: [{ name: 'X-Api-Key', secret: { set: true } }],
  http_body_encoding: 'json',
  auth: { auth_method: 'basic', basic_auth_user: 'probe', basic_auth_pass: { set: true } },
  accepted_statuscodes: ['200-299'],
  maxredirects: 10,
  ignore_tls: false,
  cache_bust: false,
}

const filled = (type: MonitorForm['type']): MonitorForm => {
  const form = emptyForm(type)
  return { ...form, name: 'Check', hostname: 'db.example.test', port: '5432', keyword: 'ok', request: { ...form.request, url: 'https://example.test' } }
}

describe('monitor form', () => {
  it('sends only the chosen type’s settings, with Kuma’s defaults', () => {
    const sent = (form: MonitorForm) =>
      Object.entries(toInput(form).settings)
        .filter(([, value]) => value !== undefined)
        .map(([key]) => key)
        .sort()
    const keys = Object.fromEntries(creatableTypes.map((type) => [type, sent(filled(type))]))
    const request = ['accepted_statuscodes', 'auth', 'cache_bust', 'expiry_notification', 'headers', 'http_body_encoding', 'ignore_tls', 'maxredirects', 'method', 'type', 'url']
    expect(keys).toEqual({
      http: request,
      keyword: [...request, 'invert_keyword', 'keyword'].sort(),
      'json-query': [...request, 'expected_value', 'json_path', 'json_path_operator', 'retry_only_on_status_code_failure'].sort(),
      port: ['hostname', 'port', 'type'],
      ping: ['hostname', 'packet_size', 'ping_count', 'ping_per_request_timeout', 'type'],
      dns: ['dns_resolve_server', 'dns_resolve_type', 'hostname', 'port', 'type'],
      push: ['type'],
      group: ['type'],
      manual: ['type'],
    })
    expect(toInput(filled('http'))).toMatchObject({
      interval: 60,
      retry_interval: undefined,
      timeout: undefined,
      max_retries: 0,
      settings: { method: 'GET', accepted_statuscodes: ['200-299'], maxredirects: 10, auth: { auth_method: 'none' } },
    })
  })

  it('keeps what was typed for one type when switching to another and back', () => {
    const form = { ...filled('keyword'), keyword: 'Welcome' }
    const switched: MonitorForm = { ...form, type: 'port' }
    const back: MonitorForm = { ...switched, type: 'keyword' }
    expect(toInput(back).settings).toMatchObject({ type: 'keyword', keyword: 'Welcome', url: 'https://example.test' })
  })

  it('reads a stored monitor and sends its sealed secrets back untouched', () => {
    const form = formFromMonitor(stored({ type: 'keyword', ...request, keyword: 'Welcome', invert_keyword: true }))
    expect(form).toMatchObject({ type: 'keyword', tags: 'prod, web', retryInterval: '30', timeout: '48', keyword: 'Welcome' })
    const input = toInput(form as MonitorForm)
    expect(input.tags).toEqual(['prod', 'web'])
    expect(input.probes).toEqual(['2'])
    expect(input.settings).toMatchObject({
      headers: [{ name: 'X-Api-Key', secret: { set: true } }],
      auth: { auth_method: 'basic', basic_auth_user: 'probe', basic_auth_pass: { set: true } },
      invert_keyword: true,
    })
  })

  it('keeps, replaces or clears a sealed secret', () => {
    expect(secretField({ set: true, mode: 'keep', value: '' })).toEqual({ set: true })
    expect(secretField({ set: true, mode: 'replace', value: 'new' })).toEqual({ value: 'new', set: false })
    expect(secretField({ set: true, mode: 'clear', value: 'ignored' })).toEqual({ clear: true, set: false })
    expect(secretField({ ...newSecret(), value: 'first' })).toEqual({ value: 'first', set: false })
  })

  it('drops header rows without a name and seals the ones marked secret', () => {
    const form = filled('http')
    form.request.headers = [
      { name: ' Accept ', value: 'application/json', secret: null },
      { name: '', value: 'stray', secret: null },
      { name: 'Authorization', value: '', secret: { ...newSecret(), value: 'Token abc' } },
    ]
    form.request.acceptedStatusCodes = '200-299, 304'
    expect(toInput(form).settings).toMatchObject({
      headers: [
        { name: 'Accept', value: 'application/json' },
        { name: 'Authorization', secret: { value: 'Token abc', set: false } },
      ],
      accepted_statuscodes: ['200-299', '304'],
    })
  })

  it('finds missing values and non-numbers per type, keyed like server field errors', () => {
    expect(validate(emptyForm('http'))).toEqual({ name: { code: 'required' } })
    expect(validate({ ...filled('keyword'), keyword: '' })).toEqual({ 'settings.keyword': { code: 'required' } })
    expect(validate({ ...filled('port'), hostname: ' ', port: '80a' })).toEqual({
      'settings.hostname': { code: 'required' },
      'settings.port': { code: 'invalid_format' },
    })
    expect(validate({ ...filled('dns'), dnsServer: '' })).toEqual({ 'settings.dns_resolve_server': { code: 'required' } })
    expect(validate({ ...filled('group'), interval: '', timeout: '1.5', retryInterval: '' })).toEqual({ interval: { code: 'required' } })
    expect(validate(filled('push'))).toEqual({})
  })

  it('holds a new push token to the server’s length', () => {
    const push = filled('push')
    expect(validate({ ...push, pushToken: 'a'.repeat(PUSH_TOKEN_MIN_CHARS - 1) })).toEqual({
      'settings.push_token': { code: 'too_short', limit: PUSH_TOKEN_MIN_CHARS },
    })
    expect(validate({ ...push, pushToken: 'a'.repeat(PUSH_TOKEN_MAX_CHARS + 1) })).toEqual({
      'settings.push_token': { code: 'too_long', limit: PUSH_TOKEN_MAX_CHARS },
    })
    expect(validate({ ...push, pushToken: newPushToken() })).toEqual({})
  })

  it('leaves an imported type it cannot run alone', () => {
    expect(formFromMonitor(stored({ type: 'unsupported', kuma_type: 'docker', reason: '' }))).toBeNull()
  })

  it('keeps a push token unless a new one is set, and builds Kuma’s push URL', () => {
    const form = formFromMonitor(stored({ type: 'push', push_token: 'abc' })) as MonitorForm
    expect(toInput(form).settings).toEqual({ type: 'push' })
    expect(toInput(form).probes).toEqual([])
    expect(toInput({ ...form, pushToken: 'fresh' }).settings).toEqual({ type: 'push', push_token: 'fresh' })
    expect(pushUrl('https://status.example.test/', 'a+b')).toBe('https://status.example.test/api/push/a%2Bb?status=up&msg=OK&ping=')
  })

  describe('probes', () => {
    const choices: ProbeChoice[] = [
      { id: '5', name: 'Frankfurt', builtin: false, status: 'online', own: true },
      { id: '1', name: 'Built-in', builtin: true, status: 'online', own: false },
      { id: '3', name: 'Vienna', builtin: false, status: 'offline', own: false },
    ]

    it('shows the built-in probe as chosen while none is stored', () => {
      expect(chosenProbes([], choices)).toEqual(['1'])
      expect(chosenProbes(['3'], choices)).toEqual(['3'])
      expect(chosenProbes([], [])).toEqual([])
    })

    it('keeps only probes on offer, in their order, and none means the built-in again', () => {
      expect(pickProbes(['3', '9', '5'], choices)).toEqual(['5', '3'])
      expect(pickProbes([], choices)).toEqual([])
      expect(chosenProbes(pickProbes([], choices), choices)).toEqual(['1'])
    })

    it('offers the quorum only for a checked type on several probes', () => {
      expect(quorumApplies({ type: 'http', probes: [] }, choices)).toBe(false)
      expect(quorumApplies({ type: 'http', probes: ['5'] }, choices)).toBe(false)
      expect(quorumApplies({ type: 'http', probes: ['5', '1'] }, choices)).toBe(true)
      expect(quorumApplies({ type: 'push', probes: ['5', '1'] }, choices)).toBe(false)
    })

    it('sends no probes for types the server runs itself', () => {
      expect(toInput({ ...filled('manual'), probes: ['5'] }).probes).toEqual([])
      expect(toInput({ ...filled('ping'), probes: ['5'] }).probes).toEqual(['5'])
    })
  })

  it('leaves channels to the node defaults until picked', () => {
    const choices: ChannelChoice[] = [
      { id: '1', name: 'Mail', provider: 'smtp', is_default: true, active: true, own: false },
      { id: '2', name: 'Discord', provider: 'discord', is_default: false, active: true, own: true },
    ]
    const form = emptyForm()
    expect(chosenChannels(form.channels, choices)).toEqual(['1'])
    expect(toInput(form).channels).toBeUndefined()
    form.channels = ['2']
    expect(chosenChannels(form.channels, choices)).toEqual(['2'])
    expect(toInput(form).channels).toEqual(['2'])
  })
})
