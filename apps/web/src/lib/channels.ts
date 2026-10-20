import type { ChannelSettings, DeliveryEvent, DeliveryState } from '~/generated'
import type { IconName } from '~/lib/icons'
import type { MessageKey } from '~/plugins/i18n'

import { isCoreProvider, type CoreProvider, type TemplateVariable } from './channel-form'

export const providerLabels: Record<CoreProvider, MessageKey> = {
  smtp: 'channels.providers.smtp',
  discord: 'channels.providers.discord',
  webhook: 'channels.providers.webhook',
  slack: 'channels.providers.slack',
  teams: 'channels.providers.teams',
  telegram: 'channels.providers.telegram',
  ntfy: 'channels.providers.ntfy',
  pushover: 'channels.providers.pushover',
}

export const providerIcons: Record<CoreProvider, IconName> = {
  smtp: 'lucide:mail',
  discord: 'lucide:message-circle',
  webhook: 'lucide:webhook',
  slack: 'lucide:hash',
  teams: 'lucide:users',
  telegram: 'lucide:send',
  ntfy: 'lucide:bell-ring',
  pushover: 'lucide:smartphone',
}

/** Kuma's provider type of stored settings, such as `smtp` or an imported `PagerDuty`. */
export const providerOf = (settings: ChannelSettings): string =>
  settings.type === 'unsupported' ? settings.kuma_type : settings.type

/** A provider's name: translated for the ones V1Per sends to, Kuma's own type for the rest. */
export const providerName = (provider: string, t: (key: MessageKey) => string): string =>
  isCoreProvider(provider) ? t(providerLabels[provider]) : provider

export const providerIcon = (provider: string): IconName => (isCoreProvider(provider) ? providerIcons[provider] : 'lucide:circle-slash')

export const deliveryStateLabels: Record<DeliveryState, MessageKey> = {
  pending: 'channels.deliveries.states.pending',
  delivered: 'channels.deliveries.states.delivered',
  failed: 'channels.deliveries.states.failed',
  suppressed: 'channels.deliveries.states.suppressed',
}

/** The dot colour. A word always goes with it. */
export const deliveryStateFill: Record<DeliveryState, string> = {
  pending: 'bg-status-pending',
  delivered: 'bg-status-up',
  failed: 'bg-status-down',
  suppressed: 'bg-status-maintenance',
}

export const deliveryEventLabels: Record<DeliveryEvent, MessageKey> = {
  monitor_down: 'channels.deliveries.events.monitor_down',
  monitor_up: 'channels.deliveries.events.monitor_up',
  certificate_expiry: 'channels.deliveries.events.certificate_expiry',
  probe_offline: 'channels.deliveries.events.probe_offline',
  test: 'channels.deliveries.events.test',
}

export const templateVariableHints: Record<TemplateVariable, MessageKey> = {
  status: 'channels.templates.variables.status',
  name: 'channels.templates.variables.name',
  hostnameOrURL: 'channels.templates.variables.hostnameOrURL',
  msg: 'channels.templates.variables.msg',
  monitorJSON: 'channels.templates.variables.monitorJSON',
  heartbeatJSON: 'channels.templates.variables.heartbeatJSON',
}
