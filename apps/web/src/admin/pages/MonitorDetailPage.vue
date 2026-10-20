<script setup lang="ts">
import { useMutation, useQuery } from '@tanstack/vue-query'
import { useLocalStorage } from '@vueuse/core'

import { queryKeys } from '~/api/query-keys'
import { monitorsResource } from '~/api/resources/monitors'
import DeliveryTable from '~/components/channels/DeliveryTable.vue'
import ChoiceGroup from '~/components/ChoiceGroup.vue'
import StatFigure from '~/components/StatFigure.vue'
import Icon from '~/components/Icon.vue'
import FilterSelect from '~/components/monitors/FilterSelect.vue'
import HeartbeatTable from '~/components/monitors/HeartbeatTable.vue'
import HistoryBars from '~/components/monitors/HistoryBars.vue'
import LatencyChart from '~/components/monitors/LatencyChart.vue'
import { formatSpan, latencyStats, quorumCount } from '~/components/monitors/monitor-detail'
import MonitorChannels from '~/components/monitors/MonitorChannels.vue'
import MonitorDetails from '~/components/monitors/MonitorDetails.vue'
import MonitorIncident from '~/components/monitors/MonitorIncident.vue'
import MonitorPages from '~/components/monitors/MonitorPages.vue'
import MonitorStatusCard from '~/components/monitors/MonitorStatusCard.vue'
import ProbeCards from '~/components/monitors/ProbeCards.vue'
import PushUrl from '~/components/monitors/PushUrl.vue'
import RecentChanges from '~/components/monitors/RecentChanges.vue'
import { Alert } from '~/components/ui/alert'
import { Button } from '~/components/ui/button'
import { EmptyState } from '~/components/ui/empty-state'
import { Panel } from '~/components/ui/panel'
import { Skeleton } from '~/components/ui/skeleton'
import { useAvailableChannels, useMonitorDeliveries } from '~/composables/useChannels'
import { useErrorMessage } from '~/composables/useErrorMessage'
import {
  HISTORY_REFRESH_MS,
  useMonitorDetail,
  useMonitorHeartbeats,
  useMonitorSeries,
  useMonitorUptime,
} from '~/composables/useMonitors'
import { useCurrentNode, useNodeDetail } from '~/composables/useNodes'
import { usePageMeta } from '~/composables/usePageMeta'
import { useStatusCardPreference } from '~/composables/usePreferences'
import { useAvailableProbes } from '~/composables/useProbes'
import { useFormats } from '~/composables/usePreferences'
import type { ManualStatus, Monitor } from '~/generated'
import { HEARTBEATS_PAGE_MAX } from '~/generated/limits'
import { formatNumber } from '~/lib/formats'
import { can } from '~/lib/access'
import { historyRangeNames, historyRanges, historySlots, isHistoryRange, totalUptime, type HistoryRange } from '~/lib/monitor-history'
import { formatLatency, formatUptime, presenceLabels, presenceOf, typeIcons, typeLabels } from '~/lib/monitor-status'
import { isReadable } from '~/lib/node-tree'
import { useI18n, type MessageKey } from '~/plugins/i18n'

/**
 * One monitor in observe mode, laid out like the node dashboard: the canvas with its status in
 * the design the reader picked, availability and response times over one time frame, each
 * probe's view, the checks and alerts; and a column with the open incident, who gets alerted, the
 * pages showing it and its recent changes. The time frame and, with several probes, one probe's
 * own results apply to the whole canvas. The status is live on the node's topic; history is read
 * again when the status changes and every minute.
 */
const { t, locale } = useI18n()
const { numberLocale } = useFormats()
const { message } = useErrorMessage()
const route = useRoute()
const router = useRouter()
const { routeId, path, tree } = useCurrentNode()
const node = useNodeDetail(routeId)

const monitorId = computed(() => (typeof route.params.monitorId === 'string' ? route.params.monitorId : null))
const live = useMonitorDetail(monitorId)
const monitor = computed(() => live.data.value?.monitor ?? null)
const state = computed(() => live.data.value?.state ?? null)
const manage = computed(() => can(node.data.value?.node, 'monitors.manage'))
usePageMeta(() => monitor.value?.name ?? t('monitors.title'))

// A link from a parent node's list may name the parent; the monitor's own node scopes the page.
watch(monitor, (current) => {
  if (current && current.node !== routeId.value) {
    void router.replace({ name: 'node-monitor', params: { nodeId: current.node, monitorId: current.id } })
  }
})

/** The monitor's node and the readable nodes above it, highest first. */
const readablePath = computed(() => path.value.filter(isReadable).map((entry) => entry.id))
const nodeName = (id: string) => tree.index.value.byId.get(id)?.name
const managesIncidents = (id: string) => can(tree.index.value.byId.get(id), 'incidents.manage')

const presence = computed(() => (monitor.value ? presenceOf({ active: monitor.value.active, status: state.value?.status }) : 'unknown'))

const probeStates = computed(() => live.data.value?.probes ?? [])
const probeChoices = useAvailableProbes(() => (probeStates.value.length > 0 ? (monitor.value?.node ?? null) : null))
/** One probe's history instead of the monitor's; empty for the monitor's own. */
const probeFilter = ref('')
const probe = computed(() => probeFilter.value || null)
const probeOptions = computed(() => probeStates.value.map((state) => ({ value: state.probe, label: state.name })))
const probeName = computed(() => probeStates.value.find((state) => state.probe === probe.value)?.name)
// A probe that leaves the monitor leaves the filter too.
watch(probeStates, (states) => {
  if (probe.value && live.data.value && !states.some((state) => state.probe === probe.value)) probeFilter.value = ''
})
/** Probe names by ID while several probes check the monitor. */
const probeLabels = computed(() =>
  probeStates.value.length > 1 ? new Map(probeStates.value.map((state) => [state.probe, state.name])) : null
)
/** Names for rows that mix probes: none while one probe's history is shown. */
const probeNames = computed(() => (probe.value ? null : probeLabels.value))
const quorumRules: Record<Monitor['quorum'], MessageKey> = {
  majority: 'monitors.detail.quorumRules.majority',
  any: 'monitors.detail.quorumRules.any',
  all: 'monitors.detail.quorumRules.all',
}
const quorumLine = computed(() => {
  const current = monitor.value
  if (!current || !state.value || probeStates.value.length < 2) return undefined
  const { count, total } = quorumCount(probeStates.value, state.value.status)
  const seen = t('monitors.detail.quorumLine', { count, total, status: t(presenceLabels[state.value.status]) })
  return `${seen}, ${t(quorumRules[current.quorum])}`
})

const uptime = useMonitorUptime(monitorId, probe)

/** Picked under Account › Appearance. */
const cardStyle = useStatusCardPreference()

/** The time frame of the whole canvas, kept in this browser. */
const storedRange = useLocalStorage<string>('v1per:monitor-range', '24h')
const range = computed({
  get: (): HistoryRange => (isHistoryRange(storedRange.value) ? storedRange.value : '24h'),
  set: (value: HistoryRange) => (storedRange.value = value),
})
const rangeLabels: Record<HistoryRange, MessageKey> = {
  '1h': 'monitors.detail.ranges.1h',
  '24h': 'monitors.detail.ranges.24h',
  '7d': 'monitors.detail.ranges.7d',
  '30d': 'monitors.detail.ranges.30d',
  '90d': 'monitors.detail.ranges.90d',
}
const rangeOptions = computed(() => historyRangeNames.map((value) => ({ value, label: t(rangeLabels[value]) })))
const series = useMonitorSeries(monitorId, range, probe)
const slots = computed(() => (series.data.value ? historySlots(series.data.value.buckets, series.data.value.window) : null))
const step = computed(() => {
  const window = series.data.value?.window
  return window ? { minute: 60_000, hour: 3_600_000, day: 86_400_000 }[historyRanges[window.range].resolution] : 60_000
})
/**
 * The time frame in figures: uptime and failed checks, then response times while timed. The
 * labels stand while the series loads, so the panel keeps its shape.
 */
const historyFigures = computed(() => {
  const data = series.data.value
  const failed = data?.buckets.reduce((sum, bucket) => sum + bucket.down, 0)
  const figures = [
    { key: 'uptime', label: t('monitors.detail.uptime'), value: data ? (formatUptime(totalUptime(data.buckets), numberLocale.value) ?? '–') : null },
    { key: 'failed', label: t('monitors.history.failed'), value: failed === undefined ? null : formatNumber(numberLocale.value, failed) },
  ]
  if (!timed.value) return figures
  const stats = data ? latencyStats(data.points) : null
  const ms = (value: number | undefined) => (value === undefined ? (data ? '–' : null) : formatLatency(value, numberLocale.value))
  return [
    ...figures,
    { key: 'avg', label: t('monitors.history.average'), value: ms(stats?.avg) },
    { key: 'p95', label: t('monitors.history.p95'), value: ms(stats?.p95) },
    { key: 'max', label: t('monitors.history.max'), value: ms(stats?.max) },
  ]
})
/** Push, group and manual monitors have no response time of their own to chart. */
const timed = computed(() => monitor.value !== null && !['group', 'manual'].includes(monitor.value.settings.type))

/** Every probe's latest checks, for the probe tiles and the recent changes. */
const recent = useQuery({
  queryKey: computed(() => [...queryKeys.monitorHistory(monitorId.value ?? ''), 'recent'] as const),
  queryFn: ({ signal }) => monitorsResource.heartbeats({ monitor: monitorId.value ?? '', limit: HEARTBEATS_PAGE_MAX }, signal),
  enabled: () => monitorId.value !== null,
  refetchInterval: HISTORY_REFRESH_MS,
  staleTime: HISTORY_REFRESH_MS,
})
const recentBeats = computed(() => recent.data.value?.heartbeats ?? null)

const heartbeats = useMonitorHeartbeats(monitorId, probe)
const beats = computed(() => (heartbeats.data.value ? heartbeats.data.value.pages.flatMap((page) => page.heartbeats) : null))

/** The server answers alerts to those who may read the channels' logs on the monitor's node. */
const alertsShown = computed(() => can(node.data.value?.node, 'channels.manage'))
const alerts = useMonitorDeliveries(monitorId, alertsShown)
const alertRows = computed(() => (alerts.data.value ? alerts.data.value.pages.flatMap((page) => page.deliveries) : null))
const channelChoices = useAvailableChannels(() => (alertsShown.value ? routeId.value : null))
const channelNames = computed(
  () => new Map((channelChoices.data.value?.channels ?? []).map((choice) => [choice.id, choice.name] as const))
)

const notice = ref<string | null>(null)
const act = useMutation({
  mutationFn: (action: () => Promise<unknown>) => action(),
  onMutate: () => (notice.value = null),
})
const togglePause = () => {
  const current = monitor.value
  if (!current) return
  act.mutate(() => (current.active ? monitorsResource.pause(current.id) : monitorsResource.resume(current.id)))
}

const manualOptions = computed(() =>
  (['up', 'down', 'pending'] as const).map((value) => ({ value, label: t(presenceLabels[value]) }))
)
const manualStatus = computed({
  get: (): ManualStatus | '' => monitor.value?.manual_status ?? '',
  set: (status: ManualStatus | '') => {
    const current = monitor.value
    if (!current || status === '') return
    act.mutate(() => monitorsResource.setManualStatus({ monitor: current.id, status }), {
      onSuccess: () => (notice.value = t('monitors.detail.manualSet', { status: t(presenceLabels[status]) })),
    })
  },
})
const clearManual = () => {
  const current = monitor.value
  if (current) act.mutate(() => monitorsResource.setManualStatus({ monitor: current.id }))
}

/** What the check reaches, in one line. */
const target = (current: Monitor): string | null => {
  const { settings } = current
  switch (settings.type) {
    case 'http':
    case 'keyword':
    case 'json-query':
      return `${settings.method} ${settings.url}`
    case 'port':
      return `${settings.hostname}:${settings.port}`
    case 'ping':
      return settings.hostname
    case 'dns':
      return `${settings.dns_resolve_type} ${settings.hostname}`
    default:
      return null
  }
}
/** The header's line: type, where the check goes and how often. */
const headline = computed(() => {
  const current = monitor.value
  if (!current) return []
  const { settings } = current
  const where = target(current)
  const every = t('monitors.detail.everySpan', { span: formatSpan(current.interval * 1000, locale.value) })
  return [t(typeLabels[settings.type]), where, settings.type === 'manual' ? null : every].filter((part) => part !== null)
})
const pushToken = computed(() => {
  const settings = monitor.value?.settings
  return settings?.type === 'push' ? (settings.push_token ?? null) : null
})
const unsupportedType = computed(() => {
  const settings = monitor.value?.settings
  return settings?.type === 'unsupported' ? settings.kuma_type : null
})
</script>

<template>
  <EmptyState
    v-if="live.error.value && !monitor"
    icon="lucide:activity"
    tone="error"
    :title="t('monitors.unavailable')"
    :description="message(live.error.value)"
    class="mx-auto mt-12 max-w-lg"
  >
    <Button
      as-child
      variant="outline"
    >
      <RouterLink :to="{ name: 'node-monitors', params: { nodeId: routeId } }">{{ t('monitors.title') }}</RouterLink>
    </Button>
  </EmptyState>

  <div
    v-else-if="!monitor"
    class="grid gap-6 xl:grid-cols-[minmax(0,1fr)_21rem]"
  >
    <div class="grid content-start gap-6">
      <Skeleton class="h-12 w-72" />
      <Skeleton class="h-28 w-full rounded-panel" />
      <Skeleton class="h-64 w-full rounded-panel" />
    </div>
    <Skeleton class="h-64 w-full rounded-panel" />
  </div>

  <div
    v-else
    class="-mx-shell-page-x -mt-shell-gutter flex flex-col xl:-mb-shell-gutter xl:min-h-0 xl:flex-[1_1_0px] xl:flex-row"
  >
    <div class="grid min-w-0 flex-1 auto-rows-max content-start gap-6 px-shell-page-x py-6 xl:min-h-0 xl:overflow-y-auto">
      <header class="flex flex-wrap items-start justify-between gap-x-6 gap-y-3">
        <div class="grid min-w-0 gap-1">
          <h1 class="text-[1.375rem]/[1.875rem] font-semibold break-words text-primary">{{ monitor.name }}</h1>
          <p class="flex min-w-0 flex-wrap items-center gap-x-1.5 text-sm text-muted">
            <Icon
              :name="typeIcons[monitor.settings.type]"
              size="14"
              aria-hidden="true"
            />
            <template
              v-for="(part, index) in headline"
              :key="index"
            >
              <span
                v-if="index > 0"
                aria-hidden="true"
              >·</span>
              <span class="min-w-0 [overflow-wrap:anywhere] select-text">{{ part }}</span>
            </template>
          </p>
        </div>
        <div class="flex max-w-full min-w-0 flex-wrap items-center gap-2">
          <FilterSelect
            v-if="probeOptions.length > 1"
            v-model="probeFilter"
            :label="t('monitors.detail.probeFilter')"
            :all-label="t('monitors.detail.allProbes')"
            :options="probeOptions"
          />
          <ChoiceGroup
            v-model="range"
            :label="t('monitors.detail.range')"
            :options="rangeOptions"
          />
          <Button
            v-if="manage && !unsupportedType"
            variant="fill"
            class="text-sm"
            :disabled="act.isPending.value"
            @click="togglePause"
          >
            <Icon :name="monitor.active ? 'lucide:pause' : 'lucide:play'" />
            {{ monitor.active ? t('monitors.detail.pause') : t('monitors.detail.resume') }}
          </Button>
          <Button
            v-if="manage"
            as-child
            variant="fill"
            class="text-sm"
          >
            <RouterLink :to="{ name: 'node-monitor-edit', params: { nodeId: monitor.node, monitorId: monitor.id } }">
              <Icon name="lucide:sliders-horizontal" />
              {{ t('monitors.detail.configure') }}
            </RouterLink>
          </Button>
        </div>
      </header>

      <div
        aria-live="polite"
        class="empty:hidden"
      >
        <Alert
          v-if="act.error.value"
          color="destructive"
        >
          <p class="text-sm">{{ message(act.error.value) }}</p>
        </Alert>
        <Alert
          v-else-if="notice"
          color="success"
        >
          <p class="text-sm">{{ notice }}</p>
        </Alert>
      </div>
      <Alert
        v-if="unsupportedType"
        color="warning"
      >
        <p class="text-sm">{{ t('monitors.detail.unsupported', { type: unsupportedType }) }}</p>
      </Alert>

      <MonitorStatusCard
        :design="cardStyle"
        :presence="presence"
        :state="state"
        :uptime="uptime.data.value"
      >
        <div
          v-if="monitor.settings.type === 'manual' && manage"
          class="flex flex-wrap items-center gap-2 border-t border-(--tone)/20 pt-4"
        >
          <span class="text-sm text-muted">{{ t('monitors.detail.manual') }}</span>
          <ChoiceGroup
            v-model="manualStatus"
            :label="t('monitors.detail.manual')"
            :options="manualOptions"
          />
          <Button
            v-if="monitor.manual_status"
            variant="ghost"
            size="sm"
            :disabled="act.isPending.value"
            @click="clearManual"
          >
            {{ t('monitors.detail.manualClear') }}
          </Button>
        </div>
      </MonitorStatusCard>

      <Panel
        v-if="monitor.settings.type === 'push'"
        id="monitor-push"
        :title="t('monitors.detail.push')"
      >
        <PushUrl
          v-if="pushToken"
          id="monitor-push-url"
          :token="pushToken"
        />
        <p
          v-else
          class="text-sm text-muted"
        >
          {{ t('monitors.editor.pushHidden') }}
        </p>
      </Panel>

      <Panel
        id="monitor-history"
        :title="t('monitors.history.title')"
        :meta="probeName"
      >
        <p
          v-if="series.error.value && !series.data.value"
          class="flex h-24 items-center justify-center text-center text-sm text-muted"
        >
          {{ message(series.error.value) }}
        </p>
        <div
          v-else
          class="grid gap-6"
        >
          <dl
            class="grid grid-cols-2 gap-x-6 gap-y-4 @lg:grid-cols-3 @4xl:grid-cols-5"
            :class="timed && 'pl-16'"
          >
            <StatFigure
              v-for="figure in historyFigures"
              :key="figure.key"
              :label="figure.label"
              :value="figure.value"
              size="xl"
            />
          </dl>
          <section
            class="grid gap-2"
            aria-labelledby="monitor-availability"
            :class="timed && 'pl-16'"
          >
            <h3
              id="monitor-availability"
              class="text-sm font-medium text-primary"
            >
              {{ t('monitors.history.availability') }}
            </h3>
            <HistoryBars
              v-if="series.data.value && slots"
              :slots="slots"
              :window="series.data.value.window"
            />
            <Skeleton
              v-else
              class="h-[3.875rem] w-full"
            />
          </section>
          <section
            v-if="timed"
            class="grid gap-2"
            aria-labelledby="monitor-latency"
          >
            <h3
              id="monitor-latency"
              class="pl-16 text-sm font-medium text-primary"
            >
              {{ t('monitors.detail.latency') }}
            </h3>
            <Skeleton
              v-if="!series.data.value"
              class="ml-16 h-[12.5rem]"
            />
            <p
              v-else-if="series.data.value.points.length === 0"
              class="flex h-[12.5rem] items-center justify-center text-center text-sm text-muted"
            >
              {{ t('monitors.detail.latencyEmpty') }}
            </p>
            <LatencyChart
              v-else
              :points="series.data.value.points"
              :from="series.data.value.window.from"
              :to="series.data.value.window.to"
              :step="step"
              compact
            />
          </section>
        </div>
      </Panel>

      <Panel
        v-if="probeStates.length > 0"
        id="monitor-probes"
        :title="t('monitors.detail.probes')"
        :meta="quorumLine"
      >
        <ProbeCards
          :states="probeStates"
          :choices="probeChoices.data.value?.probes ?? []"
          :beats="recentBeats"
          :selected="probe"
          :node-name="nodeName"
          @select="(id) => (probeFilter = id ?? '')"
        />
      </Panel>

      <Panel
        id="monitor-heartbeats"
        :title="t('monitors.detail.heartbeats')"
        :meta="probeName"
        flush
      >
        <div class="border-t border-hairline">
          <HeartbeatTable
            :heartbeats="beats"
            :probes="probeNames"
            :error="heartbeats.error.value ? message(heartbeats.error.value) : null"
          />
        </div>
        <div
          v-if="heartbeats.hasNextPage.value"
          class="border-t border-hairline p-3"
        >
          <Button
            variant="fill"
            size="sm"
            :loading="heartbeats.isFetchingNextPage.value"
            @click="heartbeats.fetchNextPage()"
          >
            {{ t('monitors.detail.showOlder') }}
          </Button>
        </div>
      </Panel>

      <Panel
        v-if="alertsShown && (monitor.channels.length > 0 || (alertRows?.length ?? 0) > 0)"
        id="monitor-alerts"
        :title="t('monitors.detail.alerts')"
        :meta="t('monitors.detail.alertsMeta')"
        flush
      >
        <div class="border-t border-hairline">
          <DeliveryTable
            :deliveries="alertRows"
            :label="t('monitors.detail.alerts')"
            :error="alerts.error.value && !alertRows ? message(alerts.error.value) : null"
            :empty="t('monitors.detail.alertsEmpty')"
            :channels="channelNames"
          />
        </div>
        <div
          v-if="alerts.hasNextPage.value"
          class="border-t border-hairline p-3"
        >
          <Button
            variant="fill"
            size="sm"
            :loading="alerts.isFetchingNextPage.value"
            @click="alerts.fetchNextPage()"
          >
            {{ t('monitors.detail.showOlderAlerts') }}
          </Button>
        </div>
      </Panel>

    </div>

    <aside
      :aria-label="t('observe.rail')"
      class="grid content-start gap-[22px] border-divider px-shell-page-x py-[18px] xl:w-84 xl:shrink-0 xl:overflow-y-auto xl:border-l xl:bg-[color-mix(in_oklch,var(--color-card)_30%,var(--color-surface))] xl:px-[18px] max-xl:border-t"
    >
      <MonitorIncident
        :monitor="monitor.id"
        :scope="readablePath[0] ?? null"
        :manages="managesIncidents"
      />
      <MonitorChannels
        :node="monitor.node"
        :node-name="nodeName(monitor.node)"
        :channels="monitor.channels"
      />
      <MonitorPages
        :monitor="monitor.id"
        :nodes="readablePath"
      />
      <RecentChanges
        :beats="recentBeats"
        :probes="probeLabels"
      />
      <MonitorDetails
        :monitor="monitor"
        :timed="timed"
      />
    </aside>
  </div>
</template>
