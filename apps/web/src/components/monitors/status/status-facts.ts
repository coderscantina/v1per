import { useNow } from '@vueuse/core'
import { computed, toValue, type MaybeRefOrGetter } from 'vue'

import { useDateFormat } from '~/composables/useDateFormat'
import { useFormats } from '~/composables/usePreferences'
import type { MonitorState, MonitorUptime } from '~/generated'
import type { IconName } from '~/lib/icons'
import { formatLatency, formatUptime, presenceColor, presenceLabels, type Presence } from '~/lib/monitor-status'
import { useI18n, type MessageKey } from '~/plugins/i18n'

import { formatSpan } from '../monitor-detail'

/** What every status card design shows, the uptime windows Kuma shows included. */
export interface StatusCardProps {
  presence: Presence
  state: MonitorState | null
  /** Absent while it loads. */
  uptime?: MonitorUptime
}

/** An HTTP check's message, "200 - OK", as its code and reason; `null` for any other message. */
export function httpResponse(message: string | null): { code: number; reason: string } | null {
  const match = message?.match(/^(\d{3}) - (.+)$/)
  return match ? { code: Number(match[1]), reason: match[2] ?? '' } : null
}

export const presenceIcons: Record<Presence, IconName> = {
  up: 'lucide:circle-check',
  down: 'lucide:circle-x',
  pending: 'lucide:circle-alert',
  maintenance: 'lucide:wrench',
  paused: 'lucide:circle-pause',
  unknown: 'lucide:circle-dashed',
}

const windows: Array<{ key: keyof MonitorUptime; label: MessageKey; long: MessageKey }> = [
  { key: 'uptime_24h', label: 'monitors.detail.ranges.24h', long: 'monitors.detail.uptime24h' },
  { key: 'uptime_7d', label: 'monitors.detail.ranges.7d', long: 'monitors.detail.uptime7d' },
  { key: 'uptime_30d', label: 'monitors.detail.ranges.30d', long: 'monitors.detail.uptime30d' },
  { key: 'uptime_1y', label: 'monitors.detail.ranges.1y', long: 'monitors.detail.uptime1y' },
]

/** The status in words and figures, formatted and ticking every 10 seconds. */
export function useStatusFacts(props: MaybeRefOrGetter<StatusCardProps>) {
  const { t, locale } = useI18n()
  const { numberLocale } = useFormats()
  const { dateTime, relative } = useDateFormat()
  const now = useNow({ interval: 10_000 })

  return computed(() => {
    const { presence, state, uptime } = toValue(props)
    return {
      tone: presenceColor[presence],
      icon: presenceIcons[presence],
      label: t(presenceLabels[presence]),
      /** How long the status has held. A paused monitor's time is when it last changed, so none. */
      held:
        state && presence !== 'paused'
          ? t('monitors.detail.forSpan', { span: formatSpan(Math.max(0, now.value.getTime() - state.changed_at), locale.value) })
          : null,
      message: state?.message ?? null,
      response: httpResponse(state?.message ?? null),
      lastCheck: state ? { text: relative(state.beat_at, now.value.getTime()), title: dateTime(state.beat_at) } : null,
      latency: state?.latency_ms === undefined ? null : formatLatency(state.latency_ms, numberLocale.value),
      windows: windows.map((window) => {
        const ratio = uptime?.[window.key]
        return {
          key: window.key,
          /** "24 h", for a figure whose section already says uptime. */
          label: t(window.label),
          /** "24 hours", for screen readers and tooltips. */
          long: t(window.long),
          ratio,
          value: uptime ? (formatUptime(ratio, numberLocale.value) ?? t('monitors.detail.noData')) : null,
        }
      }),
    }
  })
}
