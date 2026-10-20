import type {
  Channel,
  ChannelSettings,
  DiscordChannelType,
  DiscordMessageFormat,
  FieldErrorCode,
  NtfyAuth,
  SecretField,
  TelegramParseMode,
  WebhookContentType,
  WebhookMethod,
} from '~/generated'

import { newSecret, secretField, type SecretInput } from './monitor-form'

/**
 * The channel editor's state, one shape for every provider: switching the provider keeps what
 * was typed for the others, and only the chosen provider's fields are sent. Numbers stay text
 * while editing. Field errors are keyed by the argument path the server uses
 * (`settings.webhook_url`), so client and server errors land on the same field.
 */

/** Providers V1Per sends to. Imported channels of other Kuma providers are kept read-only. */
export const coreProviders = ['smtp', 'discord', 'webhook', 'slack', 'teams', 'telegram', 'ntfy', 'pushover'] as const
export type CoreProvider = (typeof coreProviders)[number]

export const isCoreProvider = (provider: string): provider is CoreProvider =>
  coreProviders.some((entry) => entry === provider)

export const discordFormats = ['normal', 'minimalist', 'custom'] as const satisfies readonly DiscordMessageFormat[]
export const discordChannelTypes = ['channel', 'create_new_forum_post', 'post_to_thread'] as const satisfies readonly DiscordChannelType[]
export const webhookMethods = ['post', 'get'] as const satisfies readonly WebhookMethod[]
export const webhookContentTypes = ['json', 'form_data', 'custom'] as const satisfies readonly WebhookContentType[]
export const telegramParseModes = ['plain', 'html', 'markdown_v2'] as const satisfies readonly TelegramParseMode[]
export const ntfyAuths = ['none', 'username_password', 'access_token'] as const satisfies readonly NtfyAuth[]
/** `tls` is implicit TLS (Kuma's `smtpSecure`), usually port 465; `starttls` upgrades when offered. */
export const smtpSecurities = ['starttls', 'tls'] as const
export type SmtpSecurity = (typeof smtpSecurities)[number]

/**
 * Kuma's Liquid template variables (kuma-notifications contract, template engine). Test,
 * certificate and domain messages have no monitor, so they see the test values.
 */
export const templateVariables = ['status', 'name', 'hostnameOrURL', 'msg', 'monitorJSON', 'heartbeatJSON'] as const
export type TemplateVariable = (typeof templateVariables)[number]

/** An example template, shown as code next to the variable list. */
export const templateExample = '{{ name }} is {{ status }}: {{ msg }}'

export interface SmtpInput {
  /** Off sends through the instance mail server; host, port and login are left out then. */
  ownServer: boolean
  host: string
  port: string
  security: SmtpSecurity
  ignoreTlsError: boolean
  ignoreStarttls: boolean
  username: string
  password: SecretInput
  from: string
  to: string
  cc: string
  bcc: string
  customSubject: string
  customBody: string
  htmlBody: boolean
  additionalHeaders: SecretInput
  dkimDomain: string
  dkimKeySelector: string
  dkimPrivateKey: SecretInput
  dkimHashAlgo: string
  dkimHeaderFieldNames: string
  dkimSkipFields: string
}

export interface DiscordInput {
  webhookUrl: SecretInput
  username: string
  prefixMessage: string
  messageFormat: DiscordMessageFormat
  messageTemplate: string
  channelType: DiscordChannelType
  threadId: string
  postName: string
  disableUrl: boolean
  suppressNotifications: boolean
}

export interface WebhookInput {
  url: SecretInput
  method: WebhookMethod
  contentType: WebhookContentType
  customBody: string
  additionalHeaders: SecretInput
}

export interface SlackInput {
  webhookUrl: SecretInput
  channel: string
  username: string
  iconEmoji: string
  channelNotify: boolean
  richMessage: boolean
  useTemplate: boolean
  template: string
  includeGroupName: boolean
}

export interface TeamsInput {
  webhookUrl: SecretInput
  enableTags: boolean
}

export interface TelegramInput {
  botToken: SecretInput
  chatId: string
  serverUrl: string
  messageThreadId: string
  sendSilently: boolean
  protectContent: boolean
  useTemplate: boolean
  template: string
  parseMode: TelegramParseMode
}

export interface NtfyInput {
  serverUrl: string
  topic: string
  priority: string
  priorityDown: string
  auth: NtfyAuth
  username: string
  password: SecretInput
  accessToken: SecretInput
  call: string
  icon: string
  useTemplate: boolean
  customTitle: string
  customMessage: string
}

export interface PushoverInput {
  userKey: SecretInput
  appToken: SecretInput
  sound: string
  soundUp: string
  priority: string
  title: string
  device: string
  ttl: string
}

export interface ChannelForm {
  provider: CoreProvider
  name: string
  isDefault: boolean
  /** Links every monitor on the node and below; a one-time action, never stored. */
  applyExisting: boolean
  active: boolean
  smtp: SmtpInput
  discord: DiscordInput
  webhook: WebhookInput
  slack: SlackInput
  teams: TeamsInput
  telegram: TelegramInput
  ntfy: NtfyInput
  pushover: PushoverInput
}

/** Kuma's defaults for a new channel. */
export function emptyChannelForm(provider: CoreProvider = 'smtp'): ChannelForm {
  return {
    provider,
    name: '',
    isDefault: false,
    applyExisting: false,
    active: true,
    smtp: {
      ownServer: false,
      host: '',
      port: '587',
      security: 'starttls',
      ignoreTlsError: false,
      ignoreStarttls: false,
      username: '',
      password: newSecret(),
      from: '',
      to: '',
      cc: '',
      bcc: '',
      customSubject: '',
      customBody: '',
      htmlBody: false,
      additionalHeaders: newSecret(),
      dkimDomain: '',
      dkimKeySelector: '',
      dkimPrivateKey: newSecret(),
      dkimHashAlgo: '',
      dkimHeaderFieldNames: '',
      dkimSkipFields: '',
    },
    discord: {
      webhookUrl: newSecret(),
      username: '',
      prefixMessage: '',
      messageFormat: 'normal',
      messageTemplate: '',
      channelType: 'channel',
      threadId: '',
      postName: '',
      disableUrl: false,
      suppressNotifications: false,
    },
    webhook: { url: newSecret(), method: 'post', contentType: 'json', customBody: '', additionalHeaders: newSecret() },
    slack: {
      webhookUrl: newSecret(),
      channel: '',
      username: '',
      iconEmoji: '',
      channelNotify: false,
      richMessage: false,
      useTemplate: false,
      template: '',
      includeGroupName: true,
    },
    teams: { webhookUrl: newSecret(), enableTags: false },
    telegram: {
      botToken: newSecret(),
      chatId: '',
      serverUrl: '',
      messageThreadId: '',
      sendSilently: false,
      protectContent: false,
      useTemplate: false,
      template: '',
      parseMode: 'plain',
    },
    ntfy: {
      serverUrl: 'https://ntfy.sh',
      topic: '',
      priority: '5',
      priorityDown: '',
      auth: 'none',
      username: '',
      password: newSecret(),
      accessToken: newSecret(),
      call: '',
      icon: '',
      useTemplate: false,
      customTitle: '',
      customMessage: '',
    },
    pushover: { userKey: newSecret(), appToken: newSecret(), sound: '', soundUp: '', priority: '', title: '', device: '', ttl: '' },
  }
}

const stored = (field: SecretField): SecretInput => ({ set: field.set, mode: field.set ? 'keep' : 'replace', value: '' })
const text = (value: string | undefined) => value ?? ''
const numberText = (value: number | undefined) => (value === undefined ? '' : String(value))

/** The editor state of a stored channel; `null` for an imported provider V1Per does not send to. */
export function formFromChannel(channel: Channel): ChannelForm | null {
  const { settings } = channel
  if (settings.type === 'unsupported') return null
  const base = emptyChannelForm(settings.type)
  const form: ChannelForm = { ...base, name: channel.name, isDefault: channel.is_default, active: channel.active }
  switch (settings.type) {
    case 'smtp':
      form.smtp = {
        ownServer: Boolean(settings.host),
        host: text(settings.host),
        port: settings.port === undefined ? base.smtp.port : String(settings.port),
        security: settings.secure ? 'tls' : 'starttls',
        ignoreTlsError: settings.ignore_tls_error ?? false,
        ignoreStarttls: settings.ignore_starttls ?? false,
        username: text(settings.username),
        password: stored(settings.password),
        from: text(settings.from),
        to: text(settings.to),
        cc: text(settings.cc),
        bcc: text(settings.bcc),
        customSubject: text(settings.custom_subject),
        customBody: text(settings.custom_body),
        htmlBody: settings.html_body ?? false,
        additionalHeaders: stored(settings.additional_headers),
        dkimDomain: text(settings.dkim_domain),
        dkimKeySelector: text(settings.dkim_key_selector),
        dkimPrivateKey: stored(settings.dkim_private_key),
        dkimHashAlgo: text(settings.dkim_hash_algo),
        dkimHeaderFieldNames: text(settings.dkim_header_field_names),
        dkimSkipFields: text(settings.dkim_skip_fields),
      }
      break
    case 'discord':
      form.discord = {
        webhookUrl: stored(settings.webhook_url),
        username: text(settings.username),
        prefixMessage: text(settings.prefix_message),
        // What Kuma's sender does: the older switch counts only without a format.
        messageFormat: settings.message_format ?? (settings.use_message_template ? 'custom' : 'normal'),
        messageTemplate: text(settings.message_template),
        channelType: settings.channel_type ?? 'channel',
        threadId: text(settings.thread_id),
        postName: text(settings.post_name),
        disableUrl: settings.disable_url ?? false,
        suppressNotifications: settings.suppress_notifications ?? false,
      }
      break
    case 'webhook':
      form.webhook = {
        url: stored(settings.url),
        method: settings.method ?? 'post',
        contentType: settings.content_type ?? 'json',
        customBody: text(settings.custom_body),
        additionalHeaders: stored(settings.additional_headers),
      }
      break
    case 'slack':
      form.slack = {
        webhookUrl: stored(settings.webhook_url),
        channel: text(settings.channel),
        username: text(settings.username),
        iconEmoji: text(settings.icon_emoji),
        channelNotify: settings.channel_notify ?? false,
        richMessage: settings.rich_message ?? false,
        useTemplate: settings.use_template ?? false,
        template: text(settings.template),
        includeGroupName: settings.include_group_name ?? true,
      }
      break
    case 'teams':
      form.teams = { webhookUrl: stored(settings.webhook_url), enableTags: settings.enable_tags ?? false }
      break
    case 'telegram':
      form.telegram = {
        botToken: stored(settings.bot_token),
        chatId: text(settings.chat_id),
        serverUrl: text(settings.server_url),
        messageThreadId: text(settings.message_thread_id),
        sendSilently: settings.send_silently ?? false,
        protectContent: settings.protect_content ?? false,
        useTemplate: settings.use_template ?? false,
        template: text(settings.template),
        parseMode: settings.parse_mode ?? 'plain',
      }
      break
    case 'ntfy':
      form.ntfy = {
        serverUrl: text(settings.server_url),
        topic: text(settings.topic),
        priority: numberText(settings.priority),
        priorityDown: numberText(settings.priority_down),
        auth: settings.auth ?? 'none',
        username: text(settings.username),
        password: stored(settings.password),
        accessToken: stored(settings.access_token),
        call: text(settings.call),
        icon: text(settings.icon),
        useTemplate: settings.use_template ?? false,
        customTitle: text(settings.custom_title),
        customMessage: text(settings.custom_message),
      }
      break
    case 'pushover':
      form.pushover = {
        userKey: stored(settings.user_key),
        appToken: stored(settings.app_token),
        sound: text(settings.sound),
        soundUp: text(settings.sound_up),
        priority: numberText(settings.priority),
        title: text(settings.title),
        device: text(settings.device),
        ttl: numberText(settings.ttl),
      }
      break
  }
  return form
}

/** Trimmed text, or absent when empty, so the server applies Kuma's default. */
const optional = (value: string) => (value.trim() === '' ? undefined : value.trim())
/** A template keeps its inner whitespace; only an empty one is left out. */
const template = (value: string) => (value.trim() === '' ? undefined : value)
/** Text as a number; `NaN` when it is not one, so validation and the server both refuse it. */
const optionalNumber = (value: string) => (value.trim() === '' ? undefined : Number(value))
/** A sealed field that is never set stays `{ set: false }` rather than sending an empty value. */
const sealed = (input: SecretInput): SecretField =>
  !input.set && input.mode === 'replace' && input.value === '' ? { set: false } : secretField(input)

/** The chosen provider's settings, without the fields of the others. */
export function toChannelSettings(form: ChannelForm): ChannelSettings {
  switch (form.provider) {
    case 'smtp': {
      const smtp = form.smtp
      const server = smtp.ownServer
      return {
        type: 'smtp',
        host: server ? optional(smtp.host) : undefined,
        port: server ? optionalNumber(smtp.port) : undefined,
        secure: server ? smtp.security === 'tls' : undefined,
        ignore_tls_error: server ? smtp.ignoreTlsError : undefined,
        ignore_starttls: server && smtp.security === 'starttls' ? smtp.ignoreStarttls : undefined,
        username: server ? optional(smtp.username) : undefined,
        password: server ? sealed(smtp.password) : { clear: smtp.password.set, set: false },
        from: optional(smtp.from),
        to: optional(smtp.to),
        cc: optional(smtp.cc),
        bcc: optional(smtp.bcc),
        custom_subject: template(smtp.customSubject),
        custom_body: template(smtp.customBody),
        html_body: smtp.htmlBody,
        additional_headers: sealed(smtp.additionalHeaders),
        dkim_domain: optional(smtp.dkimDomain),
        dkim_key_selector: optional(smtp.dkimKeySelector),
        dkim_private_key: sealed(smtp.dkimPrivateKey),
        dkim_hash_algo: optional(smtp.dkimHashAlgo),
        dkim_header_field_names: optional(smtp.dkimHeaderFieldNames),
        dkim_skip_fields: optional(smtp.dkimSkipFields),
      }
    }
    case 'discord': {
      const discord = form.discord
      return {
        type: 'discord',
        webhook_url: sealed(discord.webhookUrl),
        username: optional(discord.username),
        prefix_message: optional(discord.prefixMessage),
        message_format: discord.messageFormat,
        message_template: discord.messageFormat === 'custom' ? template(discord.messageTemplate) : undefined,
        channel_type: discord.channelType,
        thread_id: discord.channelType === 'post_to_thread' ? optional(discord.threadId) : undefined,
        post_name: discord.channelType === 'create_new_forum_post' ? optional(discord.postName) : undefined,
        disable_url: discord.disableUrl,
        suppress_notifications: discord.suppressNotifications,
      }
    }
    case 'webhook': {
      const webhook = form.webhook
      return {
        type: 'webhook',
        url: sealed(webhook.url),
        method: webhook.method,
        content_type: webhook.contentType,
        custom_body: webhook.contentType === 'custom' ? template(webhook.customBody) : undefined,
        additional_headers: sealed(webhook.additionalHeaders),
      }
    }
    case 'slack': {
      const slack = form.slack
      return {
        type: 'slack',
        webhook_url: sealed(slack.webhookUrl),
        channel: optional(slack.channel),
        username: optional(slack.username),
        icon_emoji: optional(slack.iconEmoji),
        channel_notify: slack.channelNotify,
        rich_message: slack.richMessage,
        use_template: slack.useTemplate,
        template: slack.useTemplate ? template(slack.template) : undefined,
        include_group_name: slack.includeGroupName,
      }
    }
    case 'teams':
      return { type: 'teams', webhook_url: sealed(form.teams.webhookUrl), enable_tags: form.teams.enableTags }
    case 'telegram': {
      const telegram = form.telegram
      return {
        type: 'telegram',
        bot_token: sealed(telegram.botToken),
        chat_id: optional(telegram.chatId),
        server_url: optional(telegram.serverUrl),
        message_thread_id: optional(telegram.messageThreadId),
        send_silently: telegram.sendSilently,
        protect_content: telegram.protectContent,
        use_template: telegram.useTemplate,
        template: telegram.useTemplate ? template(telegram.template) : undefined,
        parse_mode: telegram.parseMode,
      }
    }
    case 'ntfy': {
      const ntfy = form.ntfy
      return {
        type: 'ntfy',
        server_url: optional(ntfy.serverUrl),
        topic: optional(ntfy.topic),
        priority: optionalNumber(ntfy.priority),
        priority_down: optionalNumber(ntfy.priorityDown),
        auth: ntfy.auth,
        username: ntfy.auth === 'username_password' ? optional(ntfy.username) : undefined,
        password: ntfy.auth === 'username_password' ? sealed(ntfy.password) : { clear: ntfy.password.set, set: false },
        access_token: ntfy.auth === 'access_token' ? sealed(ntfy.accessToken) : { clear: ntfy.accessToken.set, set: false },
        call: optional(ntfy.call),
        icon: optional(ntfy.icon),
        use_template: ntfy.useTemplate,
        custom_title: ntfy.useTemplate ? template(ntfy.customTitle) : undefined,
        custom_message: ntfy.useTemplate ? template(ntfy.customMessage) : undefined,
      }
    }
    case 'pushover': {
      const pushover = form.pushover
      return {
        type: 'pushover',
        user_key: sealed(pushover.userKey),
        app_token: sealed(pushover.appToken),
        sound: optional(pushover.sound),
        sound_up: optional(pushover.soundUp),
        priority: optionalNumber(pushover.priority),
        title: optional(pushover.title),
        device: optional(pushover.device),
        ttl: optionalNumber(pushover.ttl),
      }
    }
  }
}

/** What `channels.create` (with the node) and `channels.update` (with the channel) both take. */
export function toChannelInput(form: ChannelForm) {
  return {
    name: form.name.trim(),
    is_default: form.isDefault,
    active: form.active,
    apply_existing: form.applyExisting || undefined,
    settings: toChannelSettings(form),
  }
}

export type ChannelFormErrors = Partial<Record<string, { code: FieldErrorCode }>>

/** A sealed value the channel cannot send without: none stored and none typed, or cleared. */
const secretMissing = (input: SecretInput) => (input.set && input.mode === 'keep' ? false : input.mode === 'clear' || input.value.trim() === '')

const isWhole = (value: string) => /^\d+$/.test(value.trim())
const isInteger = (value: string) => /^-?\d+$/.test(value.trim())

/**
 * What the browser can tell before sending: missing values and numbers that are not numbers.
 * Formats and ranges are the server's to judge; its field errors land on the same keys.
 */
export function validateChannel(form: ChannelForm): ChannelFormErrors {
  const errors: ChannelFormErrors = {}
  const required = (key: string, missing: boolean) => {
    if (missing) errors[key] = { code: 'required' }
  }
  const number = (key: string, value: string, check: (value: string) => boolean, optional = true) => {
    if (value.trim() === '') {
      if (!optional) errors[key] = { code: 'required' }
    } else if (!check(value)) errors[key] = { code: 'invalid_format' }
  }

  required('name', form.name.trim() === '')
  switch (form.provider) {
    case 'smtp': {
      const smtp = form.smtp
      if (smtp.ownServer) {
        required('settings.host', smtp.host.trim() === '')
        number('settings.port', smtp.port, isWhole, false)
      }
      required('settings.to', [smtp.to, smtp.cc, smtp.bcc].every((value) => value.trim() === ''))
      break
    }
    case 'discord':
      required('settings.webhook_url', secretMissing(form.discord.webhookUrl))
      break
    case 'webhook':
      required('settings.url', secretMissing(form.webhook.url))
      break
    case 'slack':
      required('settings.webhook_url', secretMissing(form.slack.webhookUrl))
      break
    case 'teams':
      required('settings.webhook_url', secretMissing(form.teams.webhookUrl))
      break
    case 'telegram':
      required('settings.bot_token', secretMissing(form.telegram.botToken))
      required('settings.chat_id', form.telegram.chatId.trim() === '')
      break
    case 'ntfy':
      required('settings.topic', form.ntfy.topic.trim() === '')
      number('settings.priority', form.ntfy.priority, isWhole)
      number('settings.priority_down', form.ntfy.priorityDown, isWhole)
      break
    case 'pushover':
      required('settings.user_key', secretMissing(form.pushover.userKey))
      required('settings.app_token', secretMissing(form.pushover.appToken))
      number('settings.priority', form.pushover.priority, isInteger)
      number('settings.ttl', form.pushover.ttl, isWhole)
      break
  }
  return errors
}
