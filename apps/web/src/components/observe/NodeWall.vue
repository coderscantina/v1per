<script setup lang="ts">
import { useNow } from '@vueuse/core'

import MonitorCard from '~/components/observe/MonitorCard.vue'
import { useDateFormat } from '~/composables/useDateFormat'
import { useFormats } from '~/composables/usePreferences'
import { useMonitorHourly } from '~/composables/useMonitors'
import type { MonitorSummary, TreeNode } from '~/generated'
import { countPresences, formatLatency, formatUptime, presenceFill, presenceLabels, presenceOf, presenceRank, type Presence } from '~/lib/monitor-status'
import { isProblem, monitorsIn, worstOf, type MonitorTree } from '~/lib/monitor-tree'
import { useI18n } from '~/plugins/i18n'

/**
 * A node as a wall of tiles, one per monitor, grouped by child node: dense enough for a node
 * with hundreds of monitors or a screen on the wall. Groups and tiles run worst first, so what
 * needs someone sits top left. Each group shows its status mix, mean 24 h uptime, the 95th
 * percentile of the latest response times and its newest check, and lists its problems under
 * the tiles. Down tiles blink, paused ones are hollow. The monitors directly in the node form
 * their own group. Extended, each group takes the full width and shows a card per monitor with
 * its last 24 hours, read in one request for the whole node.
 */
const props = defineProps<{ tree: MonitorTree; node: TreeNode; extended?: boolean }>()

const { t, locale } = useI18n()
const { numberLocale } = useFormats()
const { relative } = useDateFormat()
const now = useNow({ interval: 10_000 })
const hourly = useMonitorHourly(() => props.node.id, () => props.extended)
const pointsOf = computed(() => new Map((hourly.data.value?.monitors ?? []).map((entry) => [entry.monitor, entry.points])))

const MIX: Presence[] = ['down', 'pending', 'maintenance', 'up', 'unknown', 'paused']

interface Group {
  key: string
  name: string
  /** The child node's dashboard; the node's own monitors have none. */
  node: string | null
  monitors: MonitorSummary[]
}

const byPresence = (a: MonitorSummary, b: MonitorSummary) =>
  presenceRank(presenceOf(a)) - presenceRank(presenceOf(b)) || a.name.localeCompare(b.name)

const groups = computed(() => {
  const list: Group[] = props.tree.children(props.node.id).map((child) => ({
    key: child.id,
    name: child.name ?? '',
    node: child.id,
    monitors: props.tree.below(child.id),
  }))
  const direct = props.tree.monitorsOn(props.node.id).flatMap((monitor) => monitorsIn(props.tree, { kind: 'monitor', monitor }))
  if (direct.length > 0) {
    const name = props.node.name ?? ''
    list.push({ key: 'direct', name: list.length > 0 ? t('observe.wall.direct', { name }) : name, node: null, monitors: direct })
  }
  return list
    .map((group, order) => {
      const active = group.monitors.filter((monitor) => monitor.active)
      const uptimes = active.flatMap((monitor) => (monitor.uptime_24h === undefined ? [] : [monitor.uptime_24h]))
      const latencies = active.flatMap((monitor) => (monitor.latency_ms === undefined ? [] : [monitor.latency_ms])).sort((a, b) => a - b)
      const p95 = latencies[Math.min(latencies.length - 1, Math.floor(latencies.length * 0.95))]
      const counts = countPresences(group.monitors)
      return {
        ...group,
        order,
        monitors: group.monitors.toSorted(byPresence),
        presence: worstOf(group.monitors),
        counts,
        up: counts.up,
        uptime: uptimes.length > 0 ? formatUptime(uptimes.reduce((sum, value) => sum + value, 0) / uptimes.length, numberLocale.value) : null,
        p95: p95 === undefined ? null : formatLatency(p95, numberLocale.value),
        newest: Math.max(0, ...group.monitors.map((monitor) => monitor.beat_at ?? 0)),
        problems: group.monitors.filter((monitor) => isProblem(presenceOf(monitor))).toSorted(byPresence),
      }
    })
    .sort((a, b) => presenceRank(a.presence) - presenceRank(b.presence) || a.order - b.order)
})

const monitorLink = (monitor: MonitorSummary) => ({ name: 'node-monitor', params: { nodeId: monitor.node, monitorId: monitor.id } })

/** Everything a tile knows, as its tooltip and accessible name. */
function describe(monitor: MonitorSummary) {
  const parts = [monitor.name, t(presenceLabels[presenceOf(monitor)])]
  if (monitor.latency_ms !== undefined) parts.push(formatLatency(monitor.latency_ms, numberLocale.value))
  const uptime = formatUptime(monitor.uptime_24h, numberLocale.value)
  if (uptime) parts.push(t('observe.wall.uptime', { uptime }))
  if (monitor.beat_at !== undefined) parts.push(t('observe.wall.checked', { when: relative(monitor.beat_at, now.value.getTime()) }))
  return parts.join(' · ')
}

const tileClass = (monitor: MonitorSummary) => {
  const presence = presenceOf(monitor)
  if (presence === 'paused') return 'bg-transparent ring-1 ring-status-none ring-inset'
  if (presence === 'down') return [presenceFill.down, 'shadow-[0_0_10px_-1px_var(--status-down)] motion-safe:animate-pulse']
  if (presence === 'pending') return [presenceFill.pending, 'shadow-[0_0_8px_-2px_var(--status-pending)]']
  return presenceFill[presence]
}
</script>

<template>
  <ul
    class="grid w-full content-start gap-3 motion-safe:animate-in motion-safe:fade-in"
    :class="extended ? 'grid-cols-1' : 'sm:grid-cols-[repeat(auto-fill,minmax(17rem,1fr))]'"
  >
    <li
      v-for="group in groups"
      :key="group.key"
      class="grid content-start gap-2.5 rounded-panel border bg-card p-3"
      :class="
        group.presence === 'down'
          ? 'border-status-down/50 shadow-[0_0_32px_-12px_var(--status-down)]'
          : group.presence === 'pending'
            ? 'border-status-pending/40'
            : 'border-hairline'
      "
    >
      <div class="grid gap-1.5">
        <div class="flex items-center gap-2">
          <span
            class="size-2 shrink-0 rounded-full"
            :class="presenceFill[group.presence]"
            aria-hidden="true"
          />
          <RouterLink
            v-if="group.node"
            :to="{ name: 'node', params: { nodeId: group.node } }"
            class="min-w-0 flex-1 truncate rounded-sm font-semibold text-primary underline-offset-4 hover:underline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring"
          >
            {{ group.name }}
          </RouterLink>
          <span
            v-else
            class="min-w-0 flex-1 truncate font-semibold text-primary"
          >{{ group.name }}</span>
          <span class="shrink-0 text-xs text-muted tabular-nums">
            {{ t('observe.wall.up', { up: group.up, total: group.monitors.length }) }}
          </span>
        </div>
        <div
          class="flex h-1.5 gap-0.5"
          aria-hidden="true"
        >
          <span
            v-for="presence in MIX.filter((entry) => group.counts[entry] > 0)"
            :key="presence"
            class="rounded-full"
            :class="presenceFill[presence]"
            :style="{ flexGrow: group.counts[presence] }"
          />
        </div>
        <dl class="flex flex-wrap items-center gap-x-3.5 gap-y-0.5 text-xs text-muted tabular-nums">
          <div
            v-if="group.counts.down > 0"
            class="flex gap-1 font-bold text-status-down"
          >
            <dt class="sr-only">{{ t('observe.stats.down') }}</dt>
            <dd>{{ t('observe.orbit.down', { count: group.counts.down }) }}</dd>
          </div>
          <div
            v-if="group.counts.pending > 0"
            class="flex gap-1 font-bold text-status-pending"
          >
            <dt class="sr-only">{{ t('observe.stats.pending') }}</dt>
            <dd>{{ t('observe.orbit.pending', { count: group.counts.pending }) }}</dd>
          </div>
          <div
            v-if="group.uptime"
            class="flex gap-1"
          >
            <dt>{{ t('observe.wall.uptimeLabel') }}</dt>
            <dd class="font-bold text-primary">{{ group.uptime }}</dd>
          </div>
          <div
            v-if="group.p95"
            class="flex gap-1"
          >
            <dt>{{ t('observe.wall.p95Label') }}</dt>
            <dd class="font-bold text-primary">{{ group.p95 }}</dd>
          </div>
          <div
            v-if="group.newest > 0"
            class="ml-auto flex gap-1"
          >
            <dt class="sr-only">{{ t('observe.wall.newest') }}</dt>
            <dd class="font-semibold text-primary">{{ relative(group.newest, now.getTime()) }}</dd>
          </div>
        </dl>
      </div>
      <ul
        v-if="extended"
        class="grid gap-2 sm:grid-cols-[repeat(auto-fill,minmax(14rem,1fr))]"
      >
        <li
          v-for="monitor in group.monitors"
          :key="monitor.id"
          class="min-w-0"
        >
          <MonitorCard
            :monitor="monitor"
            :points="pointsOf.get(monitor.id)"
            :from="hourly.data.value?.from"
            :checked="monitor.beat_at === undefined ? undefined : relative(monitor.beat_at, now.getTime(), 'narrow')"
          />
        </li>
      </ul>
      <ul
        v-else
        class="flex flex-wrap gap-1"
      >
        <li
          v-for="monitor in group.monitors"
          :key="monitor.id"
        >
          <RouterLink
            :to="monitorLink(monitor)"
            :title="describe(monitor)"
            :aria-label="describe(monitor)"
            class="block size-5.5 rounded-[5px] transition-[filter] hover:brightness-125 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring"
            :class="tileClass(monitor)"
          />
        </li>
      </ul>
      <ul
        v-if="!extended && group.problems.length > 0"
        class="grid gap-1.5 border-t border-hairline pt-2.5"
      >
        <li
          v-for="monitor in group.problems"
          :key="monitor.id"
          class="flex items-center gap-2 text-xs"
        >
          <span
            class="size-1.5 shrink-0 rounded-full"
            :class="presenceFill[presenceOf(monitor)]"
            aria-hidden="true"
          />
          <RouterLink
            :to="monitorLink(monitor)"
            class="min-w-0 flex-1 truncate rounded-sm text-primary underline-offset-4 hover:underline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring"
          >
            {{ monitor.name }}
          </RouterLink>
          <span
            v-if="monitor.uptime_24h !== undefined"
            class="shrink-0 text-muted tabular-nums"
          >{{ formatUptime(monitor.uptime_24h, numberLocale) }}</span>
          <span
            class="shrink-0"
            :class="presenceOf(monitor) === 'down' ? 'font-medium text-status-down' : 'text-status-pending'"
          >{{ t(presenceLabels[presenceOf(monitor)]) }}</span>
        </li>
      </ul>
    </li>
  </ul>
</template>
