<script setup lang="ts">
import { useMutation } from '@tanstack/vue-query'

import { monitorsResource } from '~/api/resources/monitors'
import DeliveryTable from '~/components/channels/DeliveryTable.vue'
import ChoiceGroup from '~/components/ChoiceGroup.vue'
import Icon from '~/components/Icon.vue'
import DayBars from '~/components/monitors/DayBars.vue'
import HeartbeatTable from '~/components/monitors/HeartbeatTable.vue'
import FilterSelect from '~/components/monitors/FilterSelect.vue'
import LatencyChart from '~/components/monitors/LatencyChart.vue'
import ProbeStateTable from '~/components/monitors/ProbeStateTable.vue'
import PushUrl from '~/components/monitors/PushUrl.vue'
import StatusLabel from '~/components/monitors/StatusLabel.vue'
import PageHeader from '~/components/PageHeader.vue'
import { Alert } from '~/components/ui/alert'
import { Badge } from '~/components/ui/badge'
import { Button } from '~/components/ui/button'
import { EmptyState } from '~/components/ui/empty-state'
import { Panel } from '~/components/ui/panel'
import { Skeleton } from '~/components/ui/skeleton'
import { TableNone } from '~/components/ui/table'
import { useAvailableChannels, useMonitorDeliveries } from '~/composables/useChannels'
import { useConfirm } from '~/composables/useConfirm'
import { useDateFormat } from '~/composables/useDateFormat'
import { useErrorMessage } from '~/composables/useErrorMessage'
import {
  latencyRangeNames,
  latencyRanges,
  localTimezone,
  useMonitorDayBars,
  useMonitorDetail,
  useMonitorHeartbeats,
  useMonitorLatency,
  useMonitorUptime,
  type LatencyRange,
} from '~/composables/useMonitors'
import { useCurrentNode, useNodeDetail } from '~/composables/useNodes'
import { usePageMeta } from '~/composables/usePageMeta'
import type { ManualStatus, Monitor, MonitorUptime } from '~/generated'
import { can } from '~/lib/access'
import { formatLatency, formatUptime, presenceLabels, presenceOf, quorumLabels, typeIcons, typeLabels } from '~/lib/monitor-status'
import { useI18n, type MessageKey } from '~/plugins/i18n'

/**
 * One monitor: its status live on the node's topic, each probe's view of it, uptime over Kuma's
 * four windows, 90 daily bars, response times over a chosen range and its checks page by page.
 * With several probes, uptime, response times and checks can show one probe's own results.
 * History is read again when the status changes and every minute.
 */
const { t, locale } = useI18n()
const { message } = useErrorMessage()
const { confirm } = useConfirm()
const { dateTime } = useDateFormat()
const route = useRoute()
const router = useRouter()
const { routeId } = useCurrentNode()
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

const presence = computed(() => (monitor.value ? presenceOf({ active: monitor.value.active, status: state.value?.status }) : 'unknown'))

const probeStates = computed(() => live.data.value?.probes ?? [])
/** One probe's history instead of the monitor's; empty for the monitor's own. */
const probeFilter = ref('')
const probe = computed(() => probeFilter.value || null)
const probeOptions = computed(() => probeStates.value.map((state) => ({ value: state.probe, label: state.name })))
const probeName = computed(() => probeStates.value.find((state) => state.probe === probe.value)?.name)
// A probe that leaves the monitor leaves the filter too.
watch(probeStates, (states) => {
  if (probe.value && live.data.value && !states.some((state) => state.probe === probe.value)) probeFilter.value = ''
})
const probeNames = computed(() =>
  probeStates.value.length > 1 && !probe.value ? new Map(probeStates.value.map((state) => [state.probe, state.name])) : null
)

const uptime = useMonitorUptime(monitorId, probe)
const windows: Array<{ key: keyof MonitorUptime; label: MessageKey }> = [
  { key: 'uptime_24h', label: 'monitors.detail.uptime24h' },
  { key: 'uptime_7d', label: 'monitors.detail.uptime7d' },
  { key: 'uptime_30d', label: 'monitors.detail.uptime30d' },
  { key: 'uptime_1y', label: 'monitors.detail.uptime1y' },
]

const bars = useMonitorDayBars(monitorId)
const timezone = localTimezone()

const range = ref<LatencyRange>('24h')
const rangeLabels: Record<LatencyRange, MessageKey> = {
  '1h': 'monitors.detail.ranges.1h',
  '24h': 'monitors.detail.ranges.24h',
  '7d': 'monitors.detail.ranges.7d',
  '30d': 'monitors.detail.ranges.30d',
}
const rangeOptions = computed(() =>
  latencyRangeNames.map((value) => ({ value, label: t(rangeLabels[value]) }))
)
const latency = useMonitorLatency(monitorId, range, probe)
const latencyWindow = computed(() => {
  const to = latency.dataUpdatedAt.value || Date.now()
  const { span, resolution } = latencyRanges[range.value]
  const step = { minute: 60_000, hour: 3_600_000, day: 86_400_000 }[resolution]
  return { from: to - span, to, step }
})
/** Push, group and manual monitors have no response time of their own to chart. */
const timed = computed(() => monitor.value !== null && !['group', 'manual'].includes(monitor.value.settings.type))

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

const remove = async () => {
  const current = monitor.value
  if (!current) return
  const ok = await confirm({
    title: t('monitors.detail.deleteTitle'),
    message: t('monitors.detail.deleteConfirm', { name: current.name }),
    confirmLabel: t('monitors.detail.delete'),
    variant: 'destructive',
  })
  if (!ok) return
  act.mutate(() => monitorsResource.delete(current.id), {
    onSuccess: () => void router.replace({ name: 'node-monitors', params: { nodeId: current.node } }),
  })
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
  <div>
    <PageHeader
      :title="monitor?.name ?? t('monitors.title')"
      :description="monitor ? t(typeLabels[monitor.settings.type]) : undefined"
    >
      <template
        v-if="monitor && manage"
        #actions
      >
        <Button
          as-child
          variant="fill"
        >
          <RouterLink :to="{ name: 'node-monitor-edit', params: { nodeId: monitor.node, monitorId: monitor.id } }">
            <Icon name="lucide:pencil" />
            {{ t('monitors.detail.edit') }}
          </RouterLink>
        </Button>
        <Button
          v-if="!unsupportedType"
          variant="fill"
          :disabled="act.isPending.value"
          @click="togglePause"
        >
          <Icon :name="monitor.active ? 'lucide:pause' : 'lucide:play'" />
          {{ monitor.active ? t('monitors.detail.pause') : t('monitors.detail.resume') }}
        </Button>
        <Button
          variant="destructive"
          :disabled="act.isPending.value"
          @click="remove"
        >
          <Icon name="lucide:trash-2" />
          {{ t('monitors.detail.delete') }}
        </Button>
      </template>
    </PageHeader>

    <EmptyState
      v-if="live.error.value && !monitor"
      icon="lucide:activity"
      tone="error"
      :title="t('monitors.unavailable')"
      :description="message(live.error.value)"
      class="mx-auto max-w-lg"
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
      class="grid max-w-6xl gap-6"
    >
      <Skeleton class="h-36 w-full rounded-panel" />
      <Skeleton class="h-64 w-full rounded-panel" />
    </div>

    <div
      v-else
      class="grid max-w-6xl gap-6"
    >
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
      <Alert
        v-else-if="!monitor.active"
        color="info"
      >
        <p class="text-sm">{{ t('monitors.detail.paused') }}</p>
      </Alert>

      <div
        v-if="probeOptions.length > 1"
        class="flex flex-wrap items-center gap-2"
      >
        <span class="text-sm text-muted">{{ t('monitors.detail.probeFilter') }}</span>
        <FilterSelect
          v-model="probeFilter"
          :label="t('monitors.detail.probeFilter')"
          :all-label="t('monitors.detail.allProbes')"
          :options="probeOptions"
        />
      </div>

      <div class="grid gap-6 lg:grid-cols-[minmax(0,1fr)_minmax(0,1.5fr)]">
        <Panel
          id="monitor-status"
          :title="t('monitors.detail.status')"
        >
          <div class="grid gap-2">
            <StatusLabel
              :presence="presence"
              size="lg"
            />
            <template v-if="state">
              <p class="text-sm text-muted">
                {{ t('monitors.detail.since', { time: dateTime(state.changed_at) }) }}
                · {{ t('monitors.detail.lastCheck', { time: dateTime(state.beat_at) }) }}
              </p>
              <p
                class="text-sm break-words text-foreground"
                data-testid="monitor-message"
              >
                {{ state.message }}
                <span
                  v-if="state.latency_ms !== undefined"
                  class="text-muted tabular-nums"
                >· {{ formatLatency(state.latency_ms, locale) }}</span>
              </p>
            </template>
            <p
              v-else
              class="text-sm text-muted"
            >
              {{ t('monitors.detail.noBeat') }}
            </p>
          </div>
          <div
            v-if="monitor.settings.type === 'manual' && manage"
            class="mt-4 grid gap-2 border-t border-divider pt-4"
          >
            <p class="text-sm font-medium text-primary">{{ t('monitors.detail.manual') }}</p>
            <div class="flex flex-wrap items-center gap-2">
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
            <p class="text-xs text-muted">{{ t('monitors.detail.manualHint') }}</p>
          </div>
        </Panel>

        <Panel
          id="monitor-uptime"
          :title="t('monitors.detail.uptime')"
          :meta="probeName"
        >
          <dl class="grid grid-cols-2 gap-4 sm:grid-cols-4">
            <div
              v-for="window in windows"
              :key="window.key"
              class="grid gap-1"
            >
              <dt class="text-sm text-muted">{{ t(window.label) }}</dt>
              <dd class="text-2xl font-semibold text-primary tabular-nums">
                <Skeleton
                  v-if="uptime.isPending.value"
                  class="h-8 w-20"
                />
                <template v-else-if="formatUptime(uptime.data.value?.[window.key], locale)">
                  {{ formatUptime(uptime.data.value?.[window.key], locale) }}
                </template>
                <span
                  v-else
                  class="text-base font-normal text-muted"
                >{{ t('monitors.detail.noData') }}</span>
              </dd>
            </div>
          </dl>
          <div class="mt-5 border-t border-divider pt-4">
            <p class="mb-2 text-sm font-medium text-primary">{{ t('monitors.detail.bars') }}</p>
            <DayBars
              v-if="bars.data.value"
              :bars="bars.data.value.bars"
              :timezone="bars.data.value.timezone || timezone"
            />
            <Skeleton
              v-else
              class="h-10 w-full"
            />
          </div>
        </Panel>
      </div>

      <Panel
        v-if="monitor.probes.length > 0 || probeStates.length > 0"
        id="monitor-probes"
        :title="t('monitors.detail.probes')"
        :meta="probeStates.length > 1 ? t('monitors.detail.probesMeta', { rule: t(quorumLabels[monitor.quorum]) }) : undefined"
        flush
      >
        <div class="border-t border-hairline">
          <ProbeStateTable :states="probeStates" />
        </div>
      </Panel>

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
        v-if="timed"
        id="monitor-latency"
        :title="t('monitors.detail.latency')"
        :meta="probeName"
      >
        <template #actions>
          <ChoiceGroup
            v-model="range"
            :label="t('monitors.detail.range')"
            :options="rangeOptions"
          />
        </template>
        <Skeleton
          v-if="latency.isPending.value"
          class="h-64 w-full"
        />
        <p
          v-else-if="latency.error.value"
          class="py-10 text-center text-sm text-muted"
        >
          {{ message(latency.error.value) }}
        </p>
        <p
          v-else-if="(latency.data.value?.points.length ?? 0) === 0"
          class="py-10 text-center text-sm text-muted"
        >
          {{ t('monitors.detail.latencyEmpty') }}
        </p>
        <LatencyChart
          v-else
          :points="latency.data.value?.points ?? []"
          :from="latencyWindow.from"
          :to="latencyWindow.to"
          :step="latencyWindow.step"
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

      <Panel
        id="monitor-settings"
        :title="t('monitors.detail.settings')"
      >
        <dl class="grid gap-x-6 gap-y-3 text-sm sm:grid-cols-2 lg:grid-cols-3">
          <div
            v-if="target(monitor)"
            class="grid gap-0.5"
          >
            <dt class="text-muted">{{ t('monitors.detail.target') }}</dt>
            <dd class="font-mono text-xs break-all text-primary">{{ target(monitor) }}</dd>
          </div>
          <div class="grid gap-0.5">
            <dt class="text-muted">{{ t('monitors.columns.type') }}</dt>
            <dd class="inline-flex items-center gap-1.5 text-primary">
              <Icon
                :name="typeIcons[monitor.settings.type]"
                size="14"
                aria-hidden="true"
              />
              {{ t(typeLabels[monitor.settings.type]) }}
            </dd>
          </div>
          <div class="grid gap-0.5">
            <dt class="text-muted">{{ t('monitors.detail.interval') }}</dt>
            <dd class="text-primary tabular-nums">{{ t('monitors.detail.every', { seconds: monitor.interval }) }}</dd>
          </div>
          <div class="grid gap-0.5">
            <dt class="text-muted">{{ t('monitors.detail.retries') }}</dt>
            <dd class="text-primary tabular-nums">{{ monitor.max_retries }}</dd>
          </div>
          <div
            v-if="timed && monitor.settings.type !== 'push'"
            class="grid gap-0.5"
          >
            <dt class="text-muted">{{ t('monitors.detail.timeout') }}</dt>
            <dd class="text-primary tabular-nums">{{ t('monitors.detail.seconds', { seconds: monitor.timeout }) }}</dd>
          </div>
          <div
            v-if="monitor.upside_down"
            class="grid gap-0.5"
          >
            <dt class="text-muted">{{ t('monitors.detail.upsideDown') }}</dt>
            <dd class="text-primary">{{ t('monitors.editor.upsideDownHint') }}</dd>
          </div>
          <div class="grid gap-0.5">
            <dt class="text-muted">{{ t('monitors.detail.tags') }}</dt>
            <dd class="flex flex-wrap gap-1">
              <Badge
                v-for="tag in monitor.tags"
                :key="tag"
                variant="surface"
                size="sm"
              >
                {{ tag }}
              </Badge>
              <TableNone v-if="monitor.tags.length === 0" />
            </dd>
          </div>
          <div
            v-if="monitor.description"
            class="grid gap-0.5"
          >
            <dt class="text-muted">{{ t('monitors.editor.description') }}</dt>
            <dd class="whitespace-pre-line text-foreground">{{ monitor.description }}</dd>
          </div>
        </dl>
      </Panel>
    </div>
  </div>
</template>
