<script setup lang="ts">
import type { MonitorSummary } from '~/generated'
import { countPresences, formatLatency, formatUptime, presenceFill, presenceLabels, type Presence } from '~/lib/monitor-status'
import { useI18n, type MessageKey } from '~/plugins/i18n'

/**
 * The scope in six numbers under a bar of its status mix: healthy monitors, the mean 24 h uptime,
 * the 95th percentile of the latest response times, and how many are down, pending or in
 * maintenance. Paused monitors count nowhere.
 */
const props = defineProps<{ monitors: MonitorSummary[] }>()

const { t, locale } = useI18n()

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

const tiles = computed(() => {
  const { counts, healthy, active, uptime, p95 } = stats.value
  return [
    { label: 'observe.stats.healthy', value: healthy, of: active },
    { label: 'observe.stats.uptime', value: uptime ?? '–' },
    { label: 'observe.stats.p95', value: p95 ?? '–' },
    { label: 'observe.stats.down', value: counts.down, alert: counts.down > 0 },
    { label: 'observe.stats.pending', value: counts.pending },
    { label: 'observe.stats.maintenance', value: counts.maintenance },
  ] satisfies Array<{ label: MessageKey; value: string | number; of?: number; alert?: boolean }>
})
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
        class="rounded-xl border border-hairline bg-card px-3 py-2.5"
      >
        <dt class="text-xs text-muted">{{ t(tile.label) }}</dt>
        <dd
          class="text-xl/7 font-[650] tabular-nums"
          :class="tile.alert ? 'text-status-down' : 'text-primary'"
        >
          {{ tile.value }}<span
            v-if="tile.of !== undefined"
            class="text-[0.8125rem] font-medium text-muted"
          > / {{ tile.of }}</span>
        </dd>
      </div>
    </dl>
  </section>
</template>
