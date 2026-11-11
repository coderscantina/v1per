<script setup lang="ts">
import type { ComponentDetail, LatencyPoint, ResponseMark, ResponseRange } from '~/generated'

import { pageClientKey } from './client'
import { formatClock, formatLatency, formatShortDate, formatTime } from './format'
import { locale, t } from './messages'
import { bandPath, chartData, linePath, markSpan, nearest, ranges, scaleTop, snapshotData, type ChartData, type Frame } from './response-chart'
import { probeLabel, probeTone } from './tones'

/**
 * Under a monitor: its response times over a chosen range as a line of the averages over the
 * fastest-to-slowest band, a line per probe (for the team) or probe location (where the page
 * shows them), incidents and maintenance marked on the time axis, and the percentiles. Grant
 * holders also get each probe's last check. The snapshot's 24 hours draw at once; the full
 * answer is read when the block scrolls into view and when the range changes.
 */
const props = defineProps<{ detail: ComponentDetail; component: string; name: string }>()

const client = inject(pageClientKey, undefined)
const root = useTemplateRef('root')
const range = ref<ResponseRange>('24h')
const loaded = shallowRef<ChartData | null>(null)
const loading = ref(false)
const failed = ref(false)
/** Labels of the series switched off in the legend. */
const off = ref<ReadonlySet<string>>(new Set())
const hover = shallowRef<LatencyPoint | null>(null)

const data = computed(() => loaded.value ?? snapshotData(props.detail.latency, Date.now()))

let latest = 0
async function load(next: ResponseRange) {
  if (!client) return
  const mine = ++latest
  range.value = next
  loading.value = true
  try {
    const answer = await client.responseTimes(props.component, next)
    if (mine !== latest) return
    loaded.value = chartData(answer)
    failed.value = false
    hover.value = null
  } catch {
    if (mine !== latest) return
    failed.value = true
    range.value = data.value.range
  } finally {
    if (mine === latest) loading.value = false
  }
}

let seen: IntersectionObserver | undefined
onMounted(() => {
  if (!root.value) return
  seen = new IntersectionObserver((entries) => {
    if (!entries.some((entry) => entry.isIntersecting)) return
    seen?.disconnect()
    void load(range.value)
  })
  seen.observe(root.value)
})
onBeforeUnmount(() => seen?.disconnect())
// A new snapshot means the page changed: read the shown range again.
watch(
  () => props.detail,
  () => {
    if (loaded.value) void load(range.value)
  }
)

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
const figures = computed(() => {
  const { overall: series } = data.value
  const points = series.points
  // The snapshot carries no summary: the mean of its points stands in until the answer arrives.
  const average = series.avg_ms ?? (points.length > 0 ? points.reduce((sum, point) => sum + point.avg_ms, 0) / points.length : undefined)
  return [
    { label: t('chart.average'), value: figure(average) },
    { label: t('chart.p50'), value: figure(series.p50_ms) },
    { label: t('chart.p95'), value: figure(series.p95_ms) },
    { label: t('chart.p99'), value: figure(series.p99_ms) },
  ]
})

const span = computed(() => t(`chart.span.${data.value.range}`))
const when = (at: number) => (data.value.to - data.value.from <= 2 * 86_400_000 ? formatClock(at, locale.value) : formatShortDate(at, locale.value))

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
    avg: figure(data.value.overall.avg_ms ?? points.reduce((sum, point) => sum + point.avg_ms, 0) / points.length),
    min: figure(Math.min(...points.map((point) => point.min_ms))),
    max: figure(Math.max(...points.map((point) => point.max_ms))),
    span: span.value,
  })
  return marks.value.length > 0 ? `${text}. ${t('chart.marks', { list: marks.value.map((mark) => mark.label).join('; ') })}` : text
})

function point(event: PointerEvent) {
  const box = (event.currentTarget as HTMLElement).getBoundingClientRect()
  const at = frame.value.from + ((event.clientX - box.left) / box.width) * (frame.value.to - frame.value.from)
  hover.value = nearest(frame.value, data.value.overall.points, at) ?? null
}
const readout = computed(() => {
  const at = hover.value
  if (!at) return null
  return {
    left: ((at.bucket + frame.value.step / 2 - frame.value.from) / (frame.value.to - frame.value.from)) * 100,
    text: t('chart.readout', {
      time: formatTime(at.bucket, locale.value),
      avg: figure(at.avg_ms),
      min: figure(at.min_ms),
      max: figure(at.max_ms),
    }),
  }
})
</script>

<template>
  <div
    ref="root"
    class="grid gap-4 rounded-lg bg-fill/60 p-3 sm:grid-cols-[1fr_auto]"
    data-testid="monitor-detail"
  >
    <div class="flex min-w-0 flex-col gap-2">
      <div class="flex flex-wrap items-center justify-between gap-2">
        <p class="text-xs font-medium text-muted">{{ t('chart.title') }}</p>
        <div
          v-if="client"
          role="group"
          :aria-label="t('chart.range')"
          class="flex rounded-control bg-fill p-[3px]"
        >
          <button
            v-for="item in ranges"
            :key="item"
            type="button"
            :aria-pressed="item === range"
            :data-range="item"
            class="rounded-lg px-2 py-0.5 text-xs font-medium text-muted transition-colors duration-100 hover:text-primary aria-pressed:bg-card aria-pressed:text-primary aria-pressed:shadow-[0_0_0_1px_var(--border-strong)]"
            @click="load(item)"
          >{{ t(`chart.range.${item}`) }}</button>
        </div>
      </div>

      <div
        class="grid grid-cols-[auto_1fr] gap-x-2 gap-y-1 transition-opacity duration-100"
        :class="loading ? 'opacity-60' : ''"
      >
        <div class="flex h-20 flex-col justify-between text-right text-[11px] leading-none text-muted tabular-nums">
          <span>{{ formatLatency(frame.top, locale) }}</span>
          <span>0</span>
        </div>
        <div
          class="relative h-20 border-b border-divider"
          data-testid="response-chart"
          @pointermove="point"
          @pointerleave="hover = null"
        >
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
              stroke-width="1.75"
              vector-effect="non-scaling-stroke"
              stroke-linejoin="round"
              class="stroke-series-primary"
            />
          </svg>
          <p
            v-else
            class="absolute inset-0 flex items-center justify-center text-xs text-muted"
          >{{ summary }}</p>
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
            <span class="absolute inset-0 rounded-xs bg-(--tone) opacity-25 group-hover:opacity-40 group-focus:opacity-40" />
            <span
              class="pointer-events-none invisible absolute bottom-full z-10 mb-1 w-max max-w-56 rounded-lg border border-border bg-card px-2 py-1 text-xs text-foreground shadow-soft-sm group-hover:visible group-focus:visible"
              :class="mark.start > 50 ? 'right-0' : 'left-0'"
            >{{ mark.label }}</span>
          </span>
          <span
            v-if="readout"
            class="pointer-events-none absolute inset-y-0 w-px bg-border-strong"
            :style="{ left: `${readout.left}%` }"
          />
        </div>
        <div class="col-start-2 flex min-h-4 items-baseline justify-between gap-2 text-[11px] leading-4 text-muted tabular-nums">
          <span class="shrink-0">{{ when(frame.from) }}</span>
          <span
            v-if="readout"
            class="truncate text-foreground"
          >{{ readout.text }}</span>
          <span class="shrink-0">{{ t('chart.now') }}</span>
        </div>
      </div>

      <ul
        v-if="data.series.length > 0"
        :aria-label="t('chart.series')"
        class="flex flex-wrap items-center gap-x-3 gap-y-1 text-xs text-foreground"
      >
        <li class="inline-flex items-center gap-1.5">
          <span
            class="h-0.5 w-3 rounded-full bg-series-primary"
            aria-hidden="true"
          />
          {{ t('chart.overall') }}
        </li>
        <li
          v-for="entry in data.series"
          :key="entry.label"
        >
          <button
            type="button"
            :aria-pressed="!off.has(entry.label)"
            :data-series="entry.slot"
            class="group inline-flex items-center gap-1.5 rounded-sm text-foreground aria-[pressed=false]:text-muted aria-[pressed=false]:line-through"
            @click="toggle(entry.label)"
          >
            <span
              class="h-0.5 w-3 rounded-full bg-(--series) group-aria-[pressed=false]:opacity-30"
              aria-hidden="true"
            />
            {{ entry.label }}
            <span class="text-muted tabular-nums">{{ figure(entry.series.avg_ms) }}</span>
          </button>
        </li>
        <li
          v-if="data.hidden > 0"
          class="text-muted"
        >{{ t('chart.hidden', { n: data.hidden }) }}</li>
      </ul>

      <dl class="grid grid-cols-4 gap-2">
        <div
          v-for="item in figures"
          :key="item.label"
          class="flex min-w-0 flex-col-reverse"
        >
          <dt class="truncate text-[11px] leading-4 text-muted">{{ item.label }}</dt>
          <dd class="truncate text-sm font-semibold text-primary tabular-nums">{{ item.value }}</dd>
        </div>
      </dl>
      <p
        v-if="failed"
        role="status"
        class="text-xs text-muted"
      >{{ t('chart.error') }}</p>
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
