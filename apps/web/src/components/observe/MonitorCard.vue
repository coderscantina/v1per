<script setup lang="ts">
import Icon from '~/components/Icon.vue'
import TrendSparkline from '~/components/observe/TrendSparkline.vue'
import type { HourPoint, HourState, MonitorSummary } from '~/generated'
import { formatLatency, formatUptime, presenceFill, presenceLabels, presenceOf, typeIcons, typeLabels } from '~/lib/monitor-status'
import { useI18n } from '~/plugins/i18n'
import { useDateFormat } from '~/composables/useDateFormat'
import { useFormats } from '~/composables/usePreferences'

/**
 * One monitor on the extended wall: what it is, how it is now (status, latest response time,
 * 24 h uptime, last check) and its last 24 hours, as a response time line over a strip of hourly
 * states. Pointing at the line shows that hour in place of the latest response time.
 */
const props = defineProps<{
  monitor: MonitorSummary
  /** 24 hourly points, oldest first; absent while they load. */
  points?: HourPoint[]
  /** Start of the first point, Unix milliseconds. */
  from?: number
  /** "25 seconds ago", or absent before the first beat. */
  checked?: string
}>()

const { t, locale } = useI18n()
const { numberLocale } = useFormats()
const { format: formatDate } = useDateFormat()

const HOUR = 3_600_000
const stateFill: Record<HourState, string> = {
  up: presenceFill.up,
  maintenance: presenceFill.maintenance,
  pending: presenceFill.pending,
  down: presenceFill.down,
}

const presence = computed(() => presenceOf(props.monitor))
const hovered = ref<number | null>(null)
const hourFormat = computed(() => formatDate({ hour: '2-digit', minute: '2-digit' }))
const hourOf = (index: number) => (props.from === undefined ? '' : hourFormat.value.format(props.from + index * HOUR))

const latency = computed(() => {
  const point = hovered.value === null ? undefined : props.points?.[hovered.value]
  if (point) return { value: point.latency_ms === undefined ? '–' : formatLatency(point.latency_ms, numberLocale.value), note: hourOf(hovered.value ?? 0) }
  return { value: props.monitor.latency_ms === undefined ? '–' : formatLatency(props.monitor.latency_ms, numberLocale.value), note: t('observe.card.latest') }
})

const hourLabel = (point: HourPoint, index: number) =>
  `${hourOf(index)}: ${point.state ? t(presenceLabels[point.state]) : t('observe.card.noData')}${
    point.uptime === undefined ? '' : `, ${formatUptime(point.uptime, numberLocale.value)}`
  }`
</script>

<template>
  <article
    class="grid min-w-0 grid-cols-[minmax(0,1fr)] gap-2 rounded-xl border bg-surface p-3"
    :class="
      presence === 'down'
        ? 'border-status-down/50 shadow-[0_0_24px_-12px_var(--status-down)]'
        : presence === 'pending'
          ? 'border-status-pending/40'
          : 'border-hairline'
    "
  >
    <header class="flex items-center gap-2">
      <Icon
        :name="typeIcons[monitor.type]"
        size="15"
        class="shrink-0 text-muted"
        :aria-label="t(typeLabels[monitor.type])"
      />
      <RouterLink
        :to="{ name: 'node-monitor', params: { nodeId: monitor.node, monitorId: monitor.id } }"
        :title="monitor.name"
        class="min-w-0 flex-1 truncate rounded-sm text-sm font-semibold text-primary underline-offset-4 hover:underline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring"
      >
        {{ monitor.name }}
      </RouterLink>
      <span class="flex shrink-0 items-center gap-1.5 text-xs font-medium text-primary">
        <span
          class="size-2 rounded-full"
          :class="[presenceFill[presence], presence === 'down' && 'motion-safe:animate-pulse']"
          aria-hidden="true"
        />
        {{ t(presenceLabels[presence]) }}
      </span>
    </header>

    <dl class="flex items-end justify-between gap-3 tabular-nums">
      <div class="min-w-0">
        <dt class="truncate text-xs text-muted">{{ latency.note }}</dt>
        <dd class="truncate text-xl/7 font-[650] text-primary">{{ latency.value }}</dd>
      </div>
      <div class="grid min-w-0 shrink-0 grid-cols-[auto_auto] gap-x-2 text-xs">
        <dt class="text-right text-muted">{{ t('observe.card.uptime') }}</dt>
        <dd class="text-right font-semibold text-primary">{{ formatUptime(monitor.uptime_24h, numberLocale) ?? '–' }}</dd>
        <dt class="text-right text-muted">{{ t('observe.card.checked') }}</dt>
        <dd class="text-right font-semibold text-primary">{{ checked ?? '–' }}</dd>
      </div>
    </dl>

    <template v-if="points">
      <TrendSparkline
        :values="points.map((point) => point.latency_ms ?? null)"
        color="var(--accent-text)"
        :label="t('observe.card.latencyTrend', { name: monitor.name })"
        @hover="hovered = $event"
      />
      <ol
        class="flex h-2.5 gap-px"
        :aria-label="t('observe.card.hours', { name: monitor.name })"
      >
        <li
          v-for="(point, index) in points"
          :key="index"
          :title="hourLabel(point, index)"
          class="flex-1 rounded-[2px]"
          :class="[point.state ? stateFill[point.state] : 'bg-fill-strong', hovered === index && 'ring-1 ring-primary']"
        >
          <span class="sr-only">{{ hourLabel(point, index) }}</span>
        </li>
      </ol>
    </template>
    <div
      v-else
      class="h-[2.375rem] animate-pulse rounded-md bg-fill"
      aria-hidden="true"
    />
  </article>
</template>
