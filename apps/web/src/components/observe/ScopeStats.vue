<script setup lang="ts">
import TrendSparkline from '~/components/observe/TrendSparkline.vue'
import type { MonitorSummary, NodeId, TrendPoint } from '~/generated'
import { useScopeTrend } from '~/composables/useMonitors'
import { countPresences, formatLatency, formatUptime, presenceFill, type Presence } from '~/lib/monitor-status'
import { useI18n, type MessageKey } from '~/plugins/i18n'

/**
 * The scope in six numbers under a bar of its status mix: healthy monitors, the mean 24 h uptime,
 * the 95th percentile of the latest response times, and how many are down, pending or in
 * maintenance. Paused monitors count nowhere. Each number has its last 24 hours below it, by
 * hour; pointing at the line shows that hour in the tile. The response line is the hourly mean,
 * since the rollups keep no percentiles.
 */
const props = defineProps<{ node: NodeId; monitors: MonitorSummary[] }>()

const { t, locale } = useI18n()
const trend = useScopeTrend(() => props.node)

const MIX: Presence[] = ['up', 'maintenance', 'pending', 'down', 'unknown']

const stats = computed(() => {
  const counts = countPresences(props.monitors)
  const uptimes = props.monitors.flatMap((monitor) => (monitor.active && monitor.uptime_24h !== undefined ? [monitor.uptime_24h] : []))
  const latencies = props.monitors
    .flatMap((monitor) => (monitor.active && monitor.latency_ms !== undefined ? [monitor.latency_ms] : []))
    .sort((a, b) => a - b)
  const p95 = latencies[Math.min(latencies.length - 1, Math.floor(latencies.length * 0.95))]
  return {
    counts,
    active: props.monitors.length - counts.paused,
    healthy: counts.up + counts.maintenance,
    uptime: uptimes.length > 0 ? formatUptime(uptimes.reduce((sum, value) => sum + value, 0) / uptimes.length, locale.value) : null,
    p95: p95 === undefined ? null : formatLatency(p95, locale.value),
  }
})

interface Tile {
  label: MessageKey
  value: string | number
  of?: number
  alert?: boolean
  color: string
  /** The tile's number for one hour; `null` when the hour has none. */
  series: (point: TrendPoint) => number | null
  format: (value: number) => string
  /** Counts start at zero; rates and times span their own range. */
  fromZero?: boolean
}

const tiles = computed(() => {
  const { counts, healthy, active, uptime, p95 } = stats.value
  const count = (value: number) => String(value)
  /** Hours without a check have no count, rather than zero. */
  const counted = (point: TrendPoint, value: number) => (point.healthy + point.down + point.pending > 0 ? value : null)
  return [
    { label: 'observe.stats.healthy', value: healthy, of: active, color: 'var(--status-up)', series: (point) => counted(point, point.healthy), format: count, fromZero: true },
    {
      label: 'observe.stats.uptime',
      value: uptime ?? '–',
      color: 'var(--status-up)',
      series: (point) => point.uptime ?? null,
      format: (value) => formatUptime(value, locale.value) ?? '–',
    },
    {
      label: 'observe.stats.p95',
      value: p95 ?? '–',
      color: 'var(--accent-text)',
      series: (point) => point.latency_ms ?? null,
      format: (value) => formatLatency(value, locale.value),
    },
    {
      label: 'observe.stats.down',
      value: counts.down,
      alert: counts.down > 0,
      color: 'var(--status-down)',
      series: (point) => counted(point, point.down),
      format: count,
      fromZero: true,
    },
    { label: 'observe.stats.pending', value: counts.pending, color: 'var(--status-pending)', series: (point) => counted(point, point.pending), format: count, fromZero: true },
    {
      label: 'observe.stats.maintenance',
      value: counts.maintenance,
      color: 'var(--status-maintenance)',
      series: (point) => counted(point, point.maintenance),
      format: count,
      fromZero: true,
    },
  ] satisfies Tile[]
})

const points = computed(() => trend.data.value?.points ?? [])
/** The tile and hour under the pointer. */
const hovered = ref<{ label: MessageKey; index: number } | null>(null)
const hourFormat = computed(() => new Intl.DateTimeFormat(locale.value, { hour: '2-digit', minute: '2-digit' }))

function hoverOf(tile: Tile) {
  if (hovered.value?.label !== tile.label) return null
  const point = points.value[hovered.value.index]
  if (!point) return null
  const value = tile.series(point)
  return { value: value === null ? '–' : tile.format(value), hour: hourFormat.value.format(point.bucket) }
}

const onHover = (label: MessageKey, index: number | null) => {
  hovered.value = index === null ? null : { label, index }
}
</script>

<template>
  <section
    :aria-label="t('observe.stats.label')"
    class="grid gap-3.5"
  >
    <div
      class="flex h-2 gap-[3px]"
      aria-hidden="true"
    >
      <span
        v-for="presence in MIX.filter((entry) => stats.counts[entry] > 0)"
        :key="presence"
        class="rounded-[3px]"
        :class="presenceFill[presence]"
        :style="{ flexGrow: stats.counts[presence] }"
      />
      <span
        v-if="stats.active === 0"
        class="flex-1 rounded-[3px] bg-fill-strong"
      />
    </div>
    <dl class="grid grid-cols-2 gap-2">
      <div
        v-for="tile in tiles"
        :key="tile.label"
        class="grid gap-1 rounded-xl border border-hairline bg-card px-3 pt-2.5 pb-2"
      >
        <dt class="flex items-baseline justify-between gap-2 text-xs text-muted">
          <span class="truncate">{{ t(tile.label) }}</span>
          <span
            v-if="hoverOf(tile)"
            class="shrink-0 tabular-nums"
          >{{ hoverOf(tile)?.hour }}</span>
        </dt>
        <dd
          class="text-[1.75rem]/9 font-[650] tracking-[-0.01em] tabular-nums"
          :class="tile.alert && !hoverOf(tile) ? 'text-status-down' : 'text-primary'"
        >
          <template v-if="hoverOf(tile)">{{ hoverOf(tile)?.value }}</template>
          <template v-else>
            {{ tile.value }}<span
              v-if="tile.of !== undefined"
              class="text-sm font-medium text-muted"
            > / {{ tile.of }}</span>
          </template>
        </dd>
        <dd>
          <TrendSparkline
            :values="points.map(tile.series)"
            :color="tile.color"
            :from-zero="tile.fromZero"
            :label="t('observe.stats.trend', { name: t(tile.label) })"
            @hover="onHover(tile.label, $event)"
          />
        </dd>
      </div>
    </dl>
  </section>
</template>
