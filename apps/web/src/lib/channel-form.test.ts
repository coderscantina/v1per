import { describe, expect, it } from 'vitest'

import type { Channel, ChannelSettings } from '~/generated'

import { emptyChannelForm, formFromChannel, toChannelInput, toChannelSettings, validateChannel } from './channel-form'

const stored = (settings: ChannelSettings): Channel => ({
  id: '4',
  node: '1',
  name: 'Ops',
  is_default: true,
  active: true,
  settings,
  monitors: 2,
  revision: 5,
  created_at: 0,
  updated_at: 0,
})

describe('channel form', () => {
  it('sends only the chosen provider, and switching back keeps what was typed', () => {
    const form = emptyChannelForm('discord')
    form.name = ' Alerts '
    form.discord.webhookUrl.value = 'https://discord.test/api/webhooks/1/x'
    form.provider = 'smtp'
    form.smtp.to = 'ops@example.test'
    form.provider = 'discord'

    const input = toChannelInput(form)
    expect(input.name).toBe('Alerts')
    expect(input.settings).toMatchObject({
      type: 'discord',
      webhook_url: { value: 'https://discord.test/api/webhooks/1/x' },
      message_format: 'normal',
      channel_type: 'channel',
    })
    expect(input.settings).not.toHaveProperty('to')
    expect(input.apply_existing).toBeUndefined()
  })

  it('sends email through the instance server unless the channel has its own', () => {
    const form = emptyChannelForm('smtp')
    form.smtp.host = 'mail.example.test'
    form.smtp.to = 'ops@example.test'
    expect(toChannelSettings(form)).toMatchObject({ type: 'smtp', host: undefined, port: undefined, to: 'ops@example.test' })

    form.smtp.ownServer = true
    form.smtp.port = '465'
    form.smtp.security = 'tls'
    form.smtp.ignoreStarttls = true
    expect(toChannelSettings(form)).toMatchObject({ host: 'mail.example.test', port: 465, secure: true, ignore_starttls: undefined })
  })

  it('keeps stored secrets and reads the Discord format the way Kuma sends it', () => {
    const form = formFromChannel(
      stored({ type: 'discord', webhook_url: { set: true }, use_message_template: true, message_template: '{{ msg }}' })
    )
    expect(form?.discord.messageFormat).toBe('custom')
    expect(form?.discord.webhookUrl).toEqual({ set: true, mode: 'keep', value: '' })
    expect(form && toChannelSettings(form)).toMatchObject({ webhook_url: { set: true }, message_template: '{{ msg }}' })
  })

  it('leaves imported providers it cannot send to read-only', () => {
    expect(formFromChannel(stored({ type: 'unsupported', kuma_type: 'PagerDuty' }))).toBeNull()
  })

  it('round-trips a stored email channel', () => {
    const form = formFromChannel(
      stored({
        type: 'smtp',
        host: 'mail.example.test',
        port: 587,
        password: { set: true },
        to: 'ops@example.test',
        custom_subject: '{{ name }} {{ status }}',
        additional_headers: { set: false },
        dkim_private_key: { set: false },
      })
    )
    expect(form?.smtp).toMatchObject({ ownServer: true, port: '587', security: 'starttls' })
    expect(form && toChannelSettings(form)).toMatchObject({
      host: 'mail.example.test',
      port: 587,
      password: { set: true },
      custom_subject: '{{ name }} {{ status }}',
      additional_headers: { set: false },
    })
  })

  it('asks for what each provider cannot send without', () => {
    const email = emptyChannelForm('smtp')
    email.smtp.ownServer = true
    email.smtp.port = 'abc'
    expect(validateChannel(email)).toEqual({
      name: { code: 'required' },
      'settings.host': { code: 'required' },
      'settings.port': { code: 'invalid_format' },
      'settings.to': { code: 'required' },
    })

    const discord = formFromChannel(stored({ type: 'discord', webhook_url: { set: true } }))
    expect(discord && validateChannel(discord)).toEqual({})
    if (discord) discord.discord.webhookUrl.mode = 'clear'
    expect(discord && validateChannel(discord)).toEqual({ 'settings.webhook_url': { code: 'required' } })

    const telegram = emptyChannelForm('telegram')
    telegram.name = 'Bot'
    expect(Object.keys(validateChannel(telegram))).toEqual(['settings.bot_token', 'settings.chat_id'])
  })
})
