<script setup lang="ts">
// Adapted from holocron (84fc54ec): apps/web/src/components/admin/StorageChart.vue. Latency is
// an average line over a band from the lowest to the highest response per bucket, broken where
// buckets without up beats leave a gap.
import type { LatencyPoint } from '~/generated'
import { formatLatency } from '~/lib/monitor-status'
import { useI18n } from '~/plugins/i18n'

const props = defineProps<{
  points: LatencyPoint[]
  /** The range shown, Unix milliseconds. */
  from: number
  to: number
  /** Bucket width in milliseconds; a longer step between points is a gap. */
  step: number
}>()

const { t, locale } = useI18n()

/** The next round number at or above a positive value: one significant digit. */
const roundUp = (value: number) => {
  const magnitude = 10 ** Math.floor(Math.log10(value))
  return Math.ceil(value / magnitude) * magnitude
}

/**
 * The top of the scale. One slow outlier would flatten the average line, so a band far above the
 * averages is cut at twice the highest average instead.
 */
const top = computed(() => {
  const highestAverage = Math.max(1, ...props.points.map((point) => point.avg_ms))
  const highest = Math.max(1, ...props.points.map((point) => point.max_ms))
  return roundUp(Math.min(highest, highestAverage * 2) * 1.05)
})

const span = computed(() => Math.max(1, props.to - props.from))
const left = (x: number) => ((x - props.from) / span.value) * 100
const level = (y: number) => (Math.min(Math.max(0, y), top.value) / top.value) * 100
const at = (x: number, y: number) => `${left(x).toFixed(3)},${(100 - level(y)).toFixed(3)}`

/** Runs of buckets without a gap between them. */
const runs = computed(() => {
  const list: LatencyPoint[][] = []
  for (const point of props.points) {
    const run = list.at(-1)
    const last = run?.at(-1)
    if (run && last && point.bucket - last.bucket <= props.step * 1.5) run.push(point)
    else list.push([point])
  }
  return list
})

const averagePath = computed(() =>
  runs.value
    .filter((run) => run.length > 1)
    .map((run) => run.map((point, index) => `${index === 0 ? 'M' : 'L'}${at(point.bucket, point.avg_ms)}`).join(' '))
    .join(' ')
)
const bandPath = computed(() =>
  runs.value
    .filter((run) => run.length > 1)
    .map((run) => {
      const upper = run.map((point, index) => `${index === 0 ? 'M' : 'L'}${at(point.bucket, point.max_ms)}`)
      const lower = [...run].reverse().map((point) => `L${at(point.bucket, point.min_ms)}`)
      return `${upper.join(' ')} ${lower.join(' ')} Z`
    })
    .join(' ')
)
const lone = computed(() => runs.value.filter((run) => run.length === 1).flat())

const ticks = computed(() => [top.value, top.value / 2, 0])
const ms = (value: number) => formatLatency(value, locale.value)

const short = computed(() => span.value <= 86_400_000)
/** Axis labels are times within a day and dates beyond it; `full` is both, for the readout. */
const formatTime = (value: number, full = false) => {
  const options: Intl.DateTimeFormatOptions = full
    ? { dateStyle: 'medium', timeStyle: 'short' }
    : short.value
      ? { timeStyle: 'short' }
      : { day: 'numeric', month: 'short' }
  return new Intl.DateTimeFormat(locale.value, options).format(value)
}

const axis = computed(() => [
  { x: props.from, label: formatTime(props.from), align: '' },
  { x: (props.from + props.to) / 2, label: formatTime((props.from + props.to) / 2), align: '-translate-x-1/2' },
  { x: props.to, label: formatTime(props.to), align: '-translate-x-full' },
])

const summary = computed(() =>
  t('monitors.detail.latencySummary', {
    count: props.points.length,
    from: formatTime(props.from, true),
    to: formatTime(props.to, true),
  })
)

const plot = ref<HTMLElement | null>(null)
const hovered = ref<LatencyPoint | null>(null)
const hover = (event: PointerEvent) => {
  const box = plot.value?.getBoundingClientRect()
  if (!box || props.points.length === 0) return
  const x = props.from + ((event.clientX - box.left) / box.width) * span.value
  hovered.value = props.points.reduce((best, point) => (Math.abs(point.bucket - x) < Math.abs(best.bucket - x) ? point : best))
}
</script>

<template>
  <figure class="m-0">
    <figcaption class="sr-only">{{ summary }}</figcaption>
    <ul
      class="mb-3 flex flex-wrap items-center gap-4 text-sm text-muted"
      aria-hidden="true"
    >
      <li class="flex items-center gap-1.5">
        <span class="h-0.5 w-4 bg-series-primary" />
        {{ t('monitors.detail.average') }}
      </li>
      <li class="flex items-center gap-1.5">
        <span class="h-2.5 w-4 rounded-[2px] bg-series-projected-fill" />
        {{ t('monitors.detail.spread') }}
      </li>
      <li
        v-if="hovered"
        class="ml-auto text-primary tabular-nums"
      >
        {{ formatTime(hovered.bucket, true) }} · {{ ms(hovered.avg_ms) }}
        <span class="text-muted">({{ ms(hovered.min_ms) }} – {{ ms(hovered.max_ms) }})</span>
      </li>
    </ul>
    <div
      class="grid grid-cols-[auto_1fr] gap-x-2"
      aria-hidden="true"
    >
      <div class="relative h-56 min-w-14 text-right text-[0.8125rem] text-muted tabular-nums">
        <span
          v-for="tick in ticks"
          :key="tick"
          class="absolute right-0 block translate-y-1/2 leading-none whitespace-nowrap"
          :style="{ bottom: `${level(tick)}%` }"
        >
          {{ ms(tick) }}
        </span>
      </div>
      <div
        ref="plot"
        class="relative h-56 touch-pan-y"
        @pointermove="hover"
        @pointerleave="hovered = null"
      >
        <span
          v-for="tick in ticks"
          :key="tick"
          class="absolute inset-x-0 border-t border-series-grid"
          :style="{ bottom: `${level(tick)}%` }"
        />
        <svg
          class="absolute inset-0 size-full overflow-hidden"
          viewBox="0 0 100 100"
          preserveAspectRatio="none"
        >
          <path
            :d="bandPath"
            class="fill-series-projected-fill"
          />
          <path
            :d="averagePath"
            fill="none"
            stroke-width="2"
            stroke-linejoin="round"
            vector-effect="non-scaling-stroke"
            class="stroke-series-primary"
          />
        </svg>
        <span
          v-for="point in lone"
          :key="point.bucket"
          class="absolute size-1.5 -translate-x-1/2 translate-y-1/2 rounded-full bg-series-primary"
          :style="{ left: `${left(point.bucket)}%`, bottom: `${level(point.avg_ms)}%` }"
        />
        <template v-if="hovered">
          <span
            class="absolute inset-y-0 border-l border-border-strong"
            :style="{ left: `${left(hovered.bucket)}%` }"
          />
          <span
            class="absolute size-2.5 -translate-x-1/2 translate-y-1/2 rounded-full bg-series-primary ring-2 ring-card"
            :style="{ left: `${left(hovered.bucket)}%`, bottom: `${level(hovered.avg_ms)}%` }"
          />
        </template>
      </div>
      <span />
      <div class="relative mt-2 h-4 text-[0.8125rem] text-muted tabular-nums">
        <span
          v-for="(mark, index) in axis"
          :key="index"
          class="absolute whitespace-nowrap"
          :class="[mark.align, index === 1 && 'max-sm:hidden']"
          :style="{ left: `${left(mark.x)}%` }"
        >
          {{ mark.label }}
        </span>
      </div>
    </div>
    <div class="sr-only">
      <table>
        <caption>{{ summary }}</caption>
        <thead>
          <tr>
            <th scope="col">{{ t('monitors.detail.time') }}</th>
            <th scope="col">{{ t('monitors.detail.average') }}</th>
            <th scope="col">{{ t('monitors.detail.spread') }}</th>
          </tr>
        </thead>
        <tbody>
          <tr
            v-for="point in points"
            :key="point.bucket"
          >
            <th scope="row">{{ formatTime(point.bucket, true) }}</th>
            <td>{{ ms(point.avg_ms) }}</td>
            <td>{{ ms(point.min_ms) }} – {{ ms(point.max_ms) }}</td>
          </tr>
        </tbody>
      </table>
    </div>
  </figure>
</template>
