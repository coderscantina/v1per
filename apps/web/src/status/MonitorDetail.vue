<script setup lang="ts">
import type { ComponentDetail, ProbeView, ResponseMark, ResponseRange, ResponseSeries } from '~/generated'

import { pageClientKey } from './client'
import { formatClock, formatLatency, formatShortDate, formatTime } from './format'
import { locale, t } from './messages'
import { bandPath, chartData, down, left, linePath, markSpan, middle, nearest, scaleTop, snapshotData, type ChartData, type Frame } from './response-chart'
import { probeLabel, probeTone } from './tones'

/**
 * Under a monitor: its response times over the page's time frame as a line of the averages over
 * the fastest-to-slowest band, a line per probe (for the team) or probe location (where the page
 * shows them), incidents and maintenance marked on the time axis, and the percentiles. Below the
 * chart one table is the legend, the per-line average and, for the team, each probe's last check;
 * while the pointer is on the chart it shows the values at that time. The snapshot's 24 hours
 * draw at once; the full answer is read while the block is in view and the range is new to it.
 */
const props = defineProps<{ detail: ComponentDetail; component: string; name: string; range: ResponseRange }>()

const client = inject(pageClientKey, undefined)
const root = useTemplateRef('root')
const loaded = shallowRef<ChartData | null>(null)
const loading = ref(false)
const failed = ref(false)
/** Labels of the lines switched off in the table. */
const off = ref<ReadonlySet<string>>(new Set())
/** The bucket under the pointer. */
const hover = shallowRef<number | null>(null)

const data = computed(() => loaded.value ?? snapshotData(props.detail.latency, Date.now()))
/** What is drawn is not yet the page's range. */
const behind = computed(() => loading.value || data.value.range !== props.range)

let latest = 0
async function load(range: ResponseRange) {
  if (!client) return
  const mine = ++latest
  loading.value = true
  try {
    const answer = await client.responseTimes(props.component, range)
    if (mine !== latest) return
    loaded.value = chartData(answer)
    failed.value = false
    hover.value = null
  } catch {
    if (mine !== latest) return
    failed.value = true
  } finally {
    if (mine === latest) loading.value = false
  }
}

// Read while in view, and again after the range or the snapshot changed.
const visible = ref(false)
const stale = ref(true)
watch([() => props.range, () => props.detail], () => (stale.value = true))
watchEffect(() => {
  if (!visible.value || !stale.value) return
  stale.value = false
  void load(props.range)
})

let seen: IntersectionObserver | undefined
onMounted(() => {
  if (!root.value) return
  seen = new IntersectionObserver((entries) => (visible.value = entries.at(-1)?.isIntersecting ?? false), { rootMargin: '200px' })
  seen.observe(root.value)
})
onBeforeUnmount(() => seen?.disconnect())

const drawn = computed(() => data.value.series.filter((entry) => !off.value.has(entry.label)))
const frame = computed(
  (): Frame => ({
    from: data.value.from,
    to: data.value.to,
    step: data.value.step,
    top: scaleTop([data.value.overall.points, ...drawn.value.map((entry) => entry.series.points)]),
  })
)
const overall = computed(() => {
  const points = data.value.overall.points
  if (points.length === 0) return null
  return { band: bandPath(frame.value, points), line: linePath(frame.value, points) }
})
const lines = computed(() => drawn.value.map((entry) => ({ slot: entry.slot, label: entry.label, path: linePath(frame.value, entry.series.points) })))

function toggle(label: string) {
  const next = new Set(off.value)
  if (!next.delete(label)) next.add(label)
  off.value = next
}

const figure = (ms: number | undefined) => (ms === undefined ? t('chart.none') : formatLatency(ms, locale.value))
const mean = (series: ResponseSeries) =>
  series.avg_ms ?? (series.points.length > 0 ? series.points.reduce((sum, point) => sum + point.avg_ms, 0) / series.points.length : undefined)

const figures = computed(() => {
  const series = data.value.overall
  return [
    { label: t('chart.average'), value: figure(mean(series)) },
    { label: t('chart.p50'), value: figure(series.p50_ms) },
    { label: t('chart.p95'), value: figure(series.p95_ms) },
    { label: t('chart.p99'), value: figure(series.p99_ms) },
  ]
})

const span = computed(() => t(`chart.span.${data.value.range}`))
const short = computed(() => data.value.to - data.value.from <= 2 * 86_400_000)
const when = (at: number) => (short.value ? formatClock(at, locale.value) : formatShortDate(at, locale.value))

function markLabel(mark: ResponseMark): string {
  const title = t(mark.kind === 'incident' ? 'chart.mark.incident' : 'chart.mark.maintenance', { title: mark.title })
  const from = formatTime(mark.from, locale.value)
  const time = mark.to === undefined ? t('chart.mark.ongoing', { from }) : t('chart.mark.span', { from, to: formatTime(mark.to, locale.value) })
  return `${title}, ${time}`
}
const marks = computed(() =>
  data.value.marks.map((mark) => {
    const { start, width } = markSpan(frame.value, mark)
    return {
      key: `${mark.kind}:${mark.incident ?? mark.title}:${mark.from}`,
      tone: mark.kind === 'maintenance' ? 'maintenance' : (mark.impact ?? 'major_outage'),
      label: markLabel(mark),
      start,
      width,
    }
  })
)

/** What the picture says, for readers who do not see it. */
const summary = computed(() => {
  const points = data.value.overall.points
  if (points.length === 0) return t('chart.empty', { span: span.value })
  const text = t('chart.summary', {
    avg: figure(mean(data.value.overall)),
    min: figure(Math.min(...points.map((point) => point.min_ms))),
    max: figure(Math.max(...points.map((point) => point.max_ms))),
    span: span.value,
  })
  return marks.value.length > 0 ? `${text}. ${t('chart.marks', { list: marks.value.map((mark) => mark.label).join('; ') })}` : text
})

function point(event: PointerEvent) {
  const box = (event.currentTarget as HTMLElement).getBoundingClientRect()
  const at = frame.value.from + ((event.clientX - box.left) / box.width) * (frame.value.to - frame.value.from)
  hover.value = nearest(frame.value, data.value.overall.points, at)?.bucket ?? null
}
const at = (series: ResponseSeries, bucket: number) => series.points.find((entry) => entry.bucket === bucket)

const readout = computed(() => {
  const bucket = hover.value
  if (bucket === null) return null
  const point = at(data.value.overall, bucket)
  if (!point) return null
  const x = left(frame.value, middle(frame.value, point))
  const dots = [
    { key: 'overall', slot: 0, top: down(frame.value, point.avg_ms) },
    ...drawn.value.flatMap((entry) => {
      const own = at(entry.series, bucket)
      return own ? [{ key: entry.label, slot: entry.slot, top: down(frame.value, own.avg_ms) }] : []
    }),
  ]
  return {
    left: x,
    dots,
    time: short.value ? formatClock(bucket, locale.value) : formatShortDate(bucket, locale.value),
  }
})

/** The table's value for a line: at the pointer's time, else over the range. */
function value(series: ResponseSeries | undefined): string {
  if (!series) return t('chart.none')
  if (hover.value === null) return figure(mean(series))
  return figure(at(series, hover.value)?.avg_ms)
}

/** A table row: a probe with its line when it has one (team), or a location's line (visitors). */
interface Row {
  key: string
  name: string
  place?: string
  slot?: number
  series?: ResponseSeries
  probe?: ProbeView
}
const rows = computed((): Row[] => {
  const lines = new Map(data.value.series.map((entry) => [entry.label, entry]))
  if (props.detail.probes.length > 0) {
    return props.detail.probes.map((probe) => {
      const entry = lines.get(probe.name)
      return { key: probe.name, name: probe.name, place: entry?.series.place, slot: entry?.slot, series: entry?.series, probe }
    })
  }
  return data.value.series.map((entry) => ({ key: entry.label, name: entry.label, slot: entry.slot, series: entry.series }))
})
const team = computed(() => props.detail.probes.length > 0)

/** The stretch before the first point, when it is a sizable part of the range: the monitor was not checked yet. */
const before = computed(() => {
  const first = data.value.overall.points[0]
  if (!first) return null
  const width = left(frame.value, first.bucket)
  return width >= 30 ? { width, text: t('chart.since', { date: when(first.bucket) }) } : null
})
</script>

<template>
  <div
    ref="root"
    class="flex flex-col gap-3 rounded-lg bg-fill/60 p-3 sm:p-4"
    data-testid="monitor-detail"
  >
    <div class="flex flex-wrap items-baseline justify-between gap-x-6 gap-y-2">
      <p class="text-sm font-medium text-primary">{{ t('chart.title') }}</p>
      <dl class="grid grid-cols-4 gap-x-5 max-sm:w-full">
        <div
          v-for="item in figures"
          :key="item.label"
          class="flex min-w-0 flex-col-reverse"
        >
          <dt class="truncate text-xs text-muted">{{ item.label }}</dt>
          <dd class="truncate text-sm font-semibold text-primary tabular-nums">{{ item.value }}</dd>
        </div>
      </dl>
    </div>

    <div
      class="grid grid-cols-[auto_1fr] gap-x-2 gap-y-1.5 transition-opacity duration-150"
      :class="behind ? 'opacity-50' : ''"
    >
      <div class="flex h-24 flex-col justify-between text-right text-[11px] leading-none text-muted tabular-nums sm:h-32">
        <span class="-translate-y-1/2">{{ formatLatency(frame.top, locale) }}</span>
        <span>{{ formatLatency(frame.top / 2, locale) }}</span>
        <span class="translate-y-1/2">0</span>
      </div>
      <div
        class="relative h-24 touch-pan-y border-b border-border sm:h-32"
        data-testid="response-chart"
        @pointermove="point"
        @pointerdown="point"
        @pointerleave="hover = null"
      >
        <span
          class="pointer-events-none absolute inset-x-0 top-0 border-t border-dashed border-divider"
          aria-hidden="true"
        />
        <span
          class="pointer-events-none absolute inset-x-0 top-1/2 border-t border-dashed border-divider"
          aria-hidden="true"
        />
        <svg
          v-if="overall"
          viewBox="0 0 100 100"
          preserveAspectRatio="none"
          role="img"
          :aria-label="`${name}: ${summary}`"
          class="absolute inset-0 size-full overflow-visible"
        >
          <path
            :d="overall.band"
            class="fill-series-projected-fill"
          />
          <path
            v-for="line in lines"
            :key="line.label"
            :d="line.path"
            :data-series="line.slot"
            fill="none"
            stroke-width="1.25"
            vector-effect="non-scaling-stroke"
            stroke-linejoin="round"
            class="stroke-(--series)"
          />
          <path
            :d="overall.line"
            fill="none"
            stroke-width="2"
            vector-effect="non-scaling-stroke"
            stroke-linejoin="round"
            class="stroke-series-primary"
          />
        </svg>
        <p
          v-else
          class="absolute inset-0 flex items-center justify-center text-sm text-muted"
        >{{ summary }}</p>
        <p
          v-if="before"
          aria-hidden="true"
          class="pointer-events-none absolute inset-y-0 left-0 flex items-center justify-center truncate px-2 text-xs whitespace-nowrap text-muted"
          :style="{ width: `${before.width}%` }"
        >{{ before.text }}</p>
        <span
          v-for="mark in marks"
          :key="mark.key"
          tabindex="0"
          role="img"
          :aria-label="mark.label"
          :data-tone="mark.tone"
          data-testid="chart-mark"
          class="group absolute inset-y-0 min-w-[3px] rounded-xs outline-offset-1"
          :style="{ left: `${mark.start}%`, width: `${mark.width}%` }"
        >
          <span class="absolute inset-0 rounded-xs bg-(--tone) opacity-20 group-hover:opacity-35 group-focus:opacity-35" />
          <span
            class="pointer-events-none invisible absolute bottom-full z-10 mb-1 w-max max-w-56 rounded-lg border border-border bg-card px-2 py-1 text-xs text-foreground shadow-soft-sm group-hover:visible group-focus:visible"
            :class="mark.start > 50 ? 'right-0' : 'left-0'"
          >{{ mark.label }}</span>
        </span>
        <template v-if="readout">
          <span
            class="pointer-events-none absolute inset-y-0 w-px bg-border-strong"
            :style="{ left: `${readout.left}%` }"
          />
          <span
            v-for="dot in readout.dots"
            :key="dot.key"
            :data-series="dot.slot || undefined"
            class="pointer-events-none absolute size-2 -translate-1/2 rounded-full ring-2 ring-card"
            :class="dot.slot ? 'bg-(--series)' : 'bg-series-primary'"
            :style="{ left: `${readout.left}%`, top: `${dot.top}%` }"
          />
        </template>
      </div>
      <div class="relative col-start-2 flex h-4 items-center justify-between text-[11px] leading-4 text-muted tabular-nums">
        <span :class="readout ? 'invisible' : ''">{{ when(frame.from) }}</span>
        <span :class="readout ? 'invisible' : ''">{{ t('chart.now') }}</span>
        <span
          v-if="readout"
          class="absolute top-0 -translate-x-1/2 rounded-sm bg-card px-1.5 font-medium whitespace-nowrap text-primary shadow-soft-sm"
          :style="{ left: `clamp(2.5rem, ${readout.left}%, calc(100% - 2.5rem))` }"
        >{{ readout.time }}</span>
      </div>
    </div>

    <ul
      v-if="rows.length > 0"
      :aria-label="t('chart.series')"
      class="grid gap-x-4 text-sm"
      :class="team ? 'grid-cols-[minmax(0,1fr)_auto_auto]' : 'grid-cols-[minmax(0,1fr)_auto]'"
    >
      <li
        aria-hidden="true"
        class="col-span-full grid grid-cols-subgrid border-b border-divider pb-1.5 text-xs text-muted"
      >
        <span>{{ team ? t('chart.probe') : t('chart.location') }}</span>
        <span v-if="team"><span class="max-sm:sr-only">{{ t('chart.lastCheck') }}</span></span>
        <span class="text-right tabular-nums">{{ readout ? readout.time : t('chart.average') }}</span>
      </li>
      <li class="col-span-full grid grid-cols-subgrid items-center py-1.5">
        <span class="inline-flex min-w-0 items-center gap-2 text-foreground">
          <span
            class="h-0.5 w-3.5 shrink-0 rounded-full bg-series-primary"
            aria-hidden="true"
          />
          {{ t('chart.overall') }}
        </span>
        <span v-if="team" />
        <span class="text-right font-medium text-primary tabular-nums">{{ value(data.overall) }}</span>
      </li>
      <li
        v-for="row in rows"
        :key="row.key"
        :data-probe="row.probe ? row.name : undefined"
        :data-tone="row.probe ? probeTone[row.probe.status] : undefined"
        class="col-span-full grid grid-cols-subgrid items-center border-t border-divider py-1.5"
      >
        <button
          v-if="row.slot"
          type="button"
          :aria-pressed="!off.has(row.name)"
          :data-series="row.slot"
          class="group inline-flex min-w-0 items-center gap-2 rounded-sm text-left text-foreground aria-[pressed=false]:text-muted"
          @click="toggle(row.name)"
        >
          <span
            class="h-0.5 w-3.5 shrink-0 rounded-full bg-(--series) group-aria-[pressed=false]:opacity-30"
            aria-hidden="true"
          />
          <span class="truncate font-medium group-aria-[pressed=false]:line-through">{{ row.name }}</span>
          <span
            v-if="row.place"
            class="truncate text-muted max-sm:hidden"
          >{{ row.place }}</span>
        </button>
        <span
          v-else
          class="inline-flex min-w-0 items-center gap-2 text-foreground"
        >
          <span
            class="w-3.5 shrink-0"
            aria-hidden="true"
          />
          <span class="truncate font-medium">{{ row.name }}</span>
        </span>
        <span
          v-if="row.probe"
          class="inline-flex items-center gap-1.5 whitespace-nowrap"
        >
          <span
            class="size-2 shrink-0 rounded-full bg-(--tone)"
            aria-hidden="true"
          />
          <span class="text-foreground">{{ t(probeLabel[row.probe.status]) }}</span>
          <span class="text-muted tabular-nums max-sm:hidden">
            {{ row.probe.latency_ms === undefined
              ? t('internal.checked', { time: formatClock(row.probe.checked_at, locale) })
              : t('internal.lastCheck', { latency: formatLatency(row.probe.latency_ms, locale), time: formatClock(row.probe.checked_at, locale) }) }}
          </span>
        </span>
        <span class="text-right text-foreground tabular-nums">{{ value(row.series) }}</span>
      </li>
      <li
        v-if="data.hidden > 0 && !team"
        class="col-span-full pt-1.5 text-xs text-muted"
      >{{ t('chart.hidden', { n: data.hidden }) }}</li>
    </ul>

    <p
      v-if="failed"
      role="status"
      class="text-xs text-muted"
    >{{ t('chart.error') }}</p>
  </div>
</template>
