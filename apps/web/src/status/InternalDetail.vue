<script setup lang="ts">
import type { ComponentDetail } from '~/generated'

import { formatClock, formatLatency } from './format'
import { locale, t } from './messages'
import { probeLabel, probeTone } from './tones'

/**
 * What grant holders see under a monitor: hourly response times over 24 hours as a line of the
 * averages over the min-to-max band, and each probe's last check.
 */
const props = defineProps<{ detail: ComponentDetail; name: string }>()

const WIDTH = 300
const HEIGHT = 56

const chart = computed(() => {
  const points = [...props.detail.latency].sort((a, b) => a.bucket - b.bucket)
  const only = points[0]
  if (!only) return null
  // One hour of data draws as a level line over that hour.
  if (points.length === 1) points.push({ ...only, bucket: only.bucket + 3_600_000 })
  const first = points[0]?.bucket ?? 0
  const span = (points.at(-1)?.bucket ?? first) - first || 1
  const top = Math.max(...points.map((point) => point.max_ms), 1)
  const x = (bucket: number) => ((bucket - first) / span) * WIDTH
  // A little room at the top, so the slowest hour does not touch the edge.
  const y = (ms: number) => HEIGHT - (ms / (top * 1.1)) * HEIGHT
  const line = points.map((point, index) => `${index ? 'L' : 'M'}${x(point.bucket).toFixed(1)},${y(point.avg_ms).toFixed(1)}`)
  const upper = points.map((point) => `${x(point.bucket).toFixed(1)},${y(point.max_ms).toFixed(1)}`)
  const lower = points.toReversed().map((point) => `${x(point.bucket).toFixed(1)},${y(point.min_ms).toFixed(1)}`)
  const average = points.reduce((sum, point) => sum + point.avg_ms, 0) / points.length
  return {
    line: line.join(''),
    band: `M${upper.join('L')}L${lower.join('L')}Z`,
    summary: t('internal.latencySummary', {
      avg: formatLatency(average, locale.value),
      min: formatLatency(Math.min(...points.map((point) => point.min_ms)), locale.value),
      max: formatLatency(top, locale.value),
    }),
  }
})
</script>

<template>
  <div
    class="grid gap-4 rounded-lg bg-fill/60 p-3 sm:grid-cols-[1fr_auto]"
    data-testid="internal-detail"
  >
    <div class="flex min-w-0 flex-col gap-1.5">
      <p class="text-xs font-medium text-muted">{{ t('internal.responseTime') }}</p>
      <svg
        v-if="chart"
        :viewBox="`0 0 ${WIDTH} ${HEIGHT}`"
        preserveAspectRatio="none"
        role="img"
        :aria-label="`${name}: ${chart.summary}`"
        class="h-14 w-full overflow-visible"
      >
        <path
          :d="chart.band"
          class="fill-series-projected-fill"
        />
        <path
          :d="chart.line"
          fill="none"
          stroke-width="1.5"
          vector-effect="non-scaling-stroke"
          stroke-linejoin="round"
          class="stroke-series-primary"
        />
      </svg>
      <p
        v-if="chart"
        class="text-xs text-muted"
      >{{ chart.summary }}</p>
      <p
        v-else
        class="text-xs text-muted"
      >{{ t('internal.noLatency') }}</p>
    </div>
    <div
      v-if="detail.probes.length > 0"
      class="flex flex-col gap-1.5"
    >
      <p class="text-xs font-medium text-muted">{{ t('internal.probes') }}</p>
      <ul class="flex flex-col gap-1 text-sm">
        <li
          v-for="probe in detail.probes"
          :key="probe.name"
          :data-tone="probeTone[probe.status]"
          :data-probe="probe.name"
          class="flex items-center gap-2"
        >
          <span
            class="size-2 shrink-0 rounded-full bg-(--tone)"
            aria-hidden="true"
          />
          <span class="font-medium text-primary">{{ probe.name }}</span>
          <span class="text-foreground">{{ t(probeLabel[probe.status]) }}</span>
          <span class="text-xs text-muted">
            <template v-if="probe.latency_ms !== undefined">{{ formatLatency(probe.latency_ms, locale) }}, </template>{{ t('internal.checked', { time: formatClock(probe.checked_at, locale) }) }}
          </span>
        </li>
      </ul>
    </div>
  </div>
</template>
