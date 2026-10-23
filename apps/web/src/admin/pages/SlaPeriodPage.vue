<script setup lang="ts">
import { useMutation } from '@tanstack/vue-query'

import { slasResource } from '~/api/resources/slas'
import Icon from '~/components/Icon.vue'
import ImpactLabel from '~/components/incidents/ImpactLabel.vue'
import PageHeader from '~/components/PageHeader.vue'
import SlaStandingBadge from '~/components/slas/SlaStandingBadge.vue'
import StatFigure from '~/components/StatFigure.vue'
import { Alert } from '~/components/ui/alert'
import { Badge } from '~/components/ui/badge'
import { Button } from '~/components/ui/button'
import { EmptyState } from '~/components/ui/empty-state'
import { Panel } from '~/components/ui/panel'
import { Progress } from '~/components/ui/progress'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '~/components/ui/select'
import { Skeleton } from '~/components/ui/skeleton'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '~/components/ui/table'
import TableEmptyRow from '~/components/ui/TableEmptyRow.vue'
import { useConfirm } from '~/composables/useConfirm'
import { useDateFormat } from '~/composables/useDateFormat'
import { useErrorMessage } from '~/composables/useErrorMessage'
import { useCurrentNode, useNodeDetail } from '~/composables/useNodes'
import { usePageMeta } from '~/composables/usePageMeta'
import { useSlaFormat } from '~/composables/useSlaFormat'
import { useSlaDetail, useSlaPeriod } from '~/composables/useSlas'
import type { SlaClock } from '~/generated'
import { can } from '~/lib/access'
import { csvFileName, periodStarts, periodTitle, settled, slaPeriodCsv, todayIn } from '~/lib/sla-period'
import { useI18n } from '~/plugins/i18n'

/**
 * One period of an SLA, live: a picker over the periods since the contract start, whether the
 * result is frozen, each commitment against its target, the covered monitors and the incidents
 * with their response and resolution clocks. The report, the CSV and, for staff above the node,
 * recomputing a frozen period sit in the header.
 */
const { t, locale } = useI18n()
const { message } = useErrorMessage()
const { confirm } = useConfirm()
const { dateTime } = useDateFormat()
const format = useSlaFormat()
const route = useRoute()
const router = useRouter()
const { routeId } = useCurrentNode()
const node = useNodeDetail(routeId)
const manage = computed(() => can(node.data.value?.node, 'slas.manage'))

const slaId = computed(() => (typeof route.params.slaId === 'string' ? route.params.slaId : null))
/** The chosen period's first day; absent for the running period. */
const chosen = computed(() => (typeof route.query.period === 'string' ? route.query.period : null))

const detail = useSlaDetail(slaId)
const sla = computed(() => detail.data.value?.sla ?? null)
const live = useSlaPeriod(slaId, chosen)
const period = computed(() => live.data.value?.period ?? null)
usePageMeta(() => sla.value?.name ?? t('slas.title'))

// A link from a parent node's list may name the parent; the SLA's own node scopes the page.
watch(sla, (current) => {
  if (current && current.node !== routeId.value) {
    void router.replace({ name: 'node-sla', params: { nodeId: current.node, slaId: current.id }, query: route.query })
  }
})

const choices = computed(() => {
  const current = sla.value
  if (!current) return []
  const starts = periodStarts(current.period, current.start_date, todayIn(current.timezone))
  return starts.map((start, index) => {
    const name = periodTitle(current.period, start, locale.value)
    return { value: start, label: index === 0 ? t('slas.view.running', { title: name }) : name }
  })
})
const selected = computed(() => chosen.value ?? period.value?.start ?? choices.value[0]?.value ?? '')
const choose = (value: unknown) => {
  if (typeof value !== 'string') return
  const running = value === choices.value[0]?.value
  void router.replace({ query: running ? {} : { period: value } })
}

const commitments = computed(() => (period.value ? format.commitments(period.value) : []))
const budgetTone = (used: number) => (used >= 1 ? 'destructive' : used >= 0.75 ? 'warning' : 'default')
const hasLatency = computed(() => period.value?.monitors.some((entry) => entry.latency_ms !== undefined) ?? false)
const each = computed(() => period.value?.terms.combine === 'each')
const clockText = (clock: SlaClock) =>
  clock.elapsed_ms === undefined ? null : format.duration(clock.elapsed_ms)

const downloadCsv = () => {
  const current = period.value
  if (!current) return
  // The byte order mark makes spreadsheet apps read the file as UTF-8.
  const url = URL.createObjectURL(new Blob(['﻿', slaPeriodCsv(current)], { type: 'text/csv;charset=utf-8' }))
  const link = document.createElement('a')
  link.href = url
  link.download = csvFileName(current.terms.name, current.start)
  link.click()
  URL.revokeObjectURL(url)
}

const notice = ref<string | null>(null)
const recomputing = useMutation({
  mutationFn: (start: string) => {
    const id = slaId.value
    if (!id) throw new Error('sla not loaded')
    return slasResource.recompute({ sla: id, start })
  },
  onMutate: () => (notice.value = null),
})
const recompute = async () => {
  const current = period.value
  if (!current) return
  const name = format.title(current)
  const ok = await confirm({
    title: t('slas.view.recomputeTitle'),
    message: t('slas.view.recomputeConfirm', { title: name }),
    confirmLabel: t('slas.view.recompute'),
  })
  if (!ok) return
  recomputing.mutate(current.start, {
    onSuccess: () => {
      notice.value = t('slas.view.recomputed', { title: name })
      void live.reload()
    },
  })
}

const failed = computed(() => (detail.error.value && !sla.value ? detail.error.value : live.error.value && !period.value ? live.error.value : null))
</script>

<template>
  <div>
    <PageHeader
      :title="sla?.name ?? t('slas.title')"
      :description="period ? format.title(period) : undefined"
    >
      <template
        v-if="period && slaId"
        #actions
      >
        <Button
          as-child
          variant="ghost"
        >
          <RouterLink :to="{ name: 'node-sla-report', params: { nodeId: routeId, slaId, start: period.start } }">
            <Icon name="lucide:printer" />
            {{ t('slas.view.report') }}
          </RouterLink>
        </Button>
        <Button
          variant="ghost"
          :title="t('slas.view.csvHint')"
          @click="downloadCsv"
        >
          <Icon name="lucide:download" />
          {{ t('slas.view.csv') }}
        </Button>
        <Button
          v-if="manage && period.frozen_at !== undefined"
          variant="ghost"
          :disabled="recomputing.isPending.value"
          @click="recompute"
        >
          <Icon name="lucide:refresh-cw" />
          {{ t('slas.view.recompute') }}
        </Button>
        <Button
          v-if="manage"
          as-child
          variant="ghost"
          size="icon"
        >
          <RouterLink
            :to="{ name: 'node-sla-edit', params: { nodeId: routeId, slaId } }"
            :aria-label="t('slas.view.edit')"
            :title="t('slas.view.edit')"
          >
            <Icon name="lucide:pencil" />
          </RouterLink>
        </Button>
      </template>
    </PageHeader>

    <EmptyState
      v-if="failed"
      icon="lucide:file-check"
      tone="error"
      :title="t('slas.unavailable')"
      :description="message(failed)"
      class="mx-auto max-w-lg"
    >
      <Button
        as-child
        variant="outline"
      >
        <RouterLink :to="{ name: 'node-slas', params: { nodeId: routeId } }">{{ t('slas.title') }}</RouterLink>
      </Button>
    </EmptyState>

    <div
      v-else-if="!period || !sla"
      class="grid max-w-6xl gap-4"
    >
      <Skeleton class="h-9 w-72" />
      <Skeleton class="h-44 w-full rounded-panel" />
      <Skeleton class="h-56 w-full rounded-panel" />
    </div>

    <div
      v-else
      class="grid max-w-6xl gap-4"
    >
      <div
        aria-live="polite"
        class="empty:hidden"
      >
        <Alert
          v-if="recomputing.error.value"
          color="destructive"
        >
          <p class="text-sm">{{ message(recomputing.error.value) }}</p>
        </Alert>
        <Alert
          v-else-if="notice"
          color="success"
        >
          <p class="text-sm">{{ notice }}</p>
        </Alert>
      </div>

      <div class="flex flex-wrap items-center gap-x-3 gap-y-2">
        <Select
          :model-value="selected"
          @update:model-value="choose"
        >
          <SelectTrigger
            class="w-64 font-normal max-sm:w-full"
            :aria-label="t('slas.view.period')"
          >
            <SelectValue>{{ choices.find((choice) => choice.value === selected)?.label ?? format.title(period) }}</SelectValue>
          </SelectTrigger>
          <SelectContent>
            <SelectItem
              v-for="choice in choices"
              :key="choice.value"
              :value="choice.value"
            >
              {{ choice.label }}
            </SelectItem>
          </SelectContent>
        </Select>
        <Badge
          v-if="period.frozen_at !== undefined"
          variant="secondary"
          size="sm"
          :title="t('slas.view.frozenAt', { time: dateTime(period.frozen_at) })"
          data-period-state="frozen"
        >
          <Icon
            name="lucide:lock"
            size="12"
            class="mr-1"
          />
          {{ t('slas.view.frozen') }}
        </Badge>
        <Badge
          v-else
          variant="info"
          size="sm"
          :title="t('slas.view.liveHint')"
          data-period-state="live"
        >
          {{ t('slas.view.live') }}
        </Badge>
        <span class="text-sm text-muted">{{ format.range(period) }}</span>
      </div>

      <Panel
        id="sla-commitments"
        :title="t('slas.view.commitments')"
      >
        <dl
          v-if="commitments.length > 0"
          class="grid gap-3 sm:grid-cols-2 xl:grid-cols-4"
        >
          <div
            v-for="entry in commitments"
            :key="entry.key"
            class="grid content-start gap-2.5 rounded-lg border border-hairline p-3.5"
            :data-commitment="entry.key"
          >
            <div class="flex items-start justify-between gap-2">
              <StatFigure
                :label="entry.label"
                :value="entry.actual"
              >
                <span
                  v-if="entry.actual === null"
                  class="font-normal text-muted"
                >{{ t('slas.view.noData') }}</span>
              </StatFigure>
              <SlaStandingBadge :standing="entry.outcome" />
            </div>
            <Progress
              v-if="entry.budget !== undefined"
              :model-value="entry.budget * 100"
              :variant="budgetTone(entry.budget)"
              :aria-label="t('slas.view.budget')"
              class="h-1.5"
            />
            <div class="grid gap-0.5 text-sm text-muted">
              <p class="text-foreground">{{ t('slas.view.target', { value: entry.target }) }}</p>
              <p v-if="entry.budget !== undefined">{{ t('slas.view.budgetUsed', { value: format.share(entry.budget) }) }}</p>
              <p
                v-for="note in entry.notes"
                :key="note"
              >
                {{ note }}
              </p>
            </div>
          </div>
        </dl>
        <p
          v-else
          class="text-sm text-muted"
        >
          {{ t('slas.view.noTargets') }}
        </p>
      </Panel>

      <Panel
        id="sla-monitors"
        :title="t('slas.view.monitors')"
        :meta="String(period.monitors.length)"
        flush
      >
        <div class="overflow-x-auto">
          <Table
            bare
            :label="t('slas.view.monitors')"
          >
            <TableHeader>
              <TableRow>
                <TableHead>{{ t('slas.view.monitor') }}</TableHead>
                <TableHead numeric>{{ t('slas.view.uptime') }}</TableHead>
                <TableHead
                  v-if="each"
                  class="max-md:hidden"
                >
                  {{ t('slas.view.budget') }}
                </TableHead>
                <TableHead
                  v-if="hasLatency"
                  numeric
                  class="max-sm:hidden"
                >
                  {{ t('slas.view.avgLatency') }}
                </TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              <TableEmptyRow
                v-if="period.monitors.length === 0"
                :colspan="4"
                :label="t('slas.view.noMonitors')"
              />
              <TableRow
                v-for="entry in period.monitors"
                :key="entry.monitor"
                :data-sla-monitor="entry.name"
              >
                <TableCell
                  kind="subject"
                  class="max-w-0 min-w-40"
                >
                  <RouterLink
                    :to="{ name: 'node-monitor', params: { nodeId: routeId, monitorId: entry.monitor } }"
                    class="block truncate rounded-sm underline-offset-4 hover:underline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring"
                  >
                    {{ entry.name }}
                  </RouterLink>
                </TableCell>
                <TableCell kind="numeric">
                  <span class="inline-flex items-center justify-end gap-2">
                    <span v-if="entry.availability !== undefined">{{ format.percent(entry.availability) }}</span>
                    <span
                      v-else
                      class="text-muted"
                    >{{ t('slas.view.noData') }}</span>
                    <SlaStandingBadge
                      v-if="settled(entry.availability_outcome)"
                      :standing="entry.availability_outcome"
                    />
                  </span>
                </TableCell>
                <TableCell
                  v-if="each"
                  class="tabular-nums max-md:hidden"
                >
                  <span
                    v-if="entry.budget_used !== undefined"
                    class="flex items-center gap-2"
                  >
                    <Progress
                      :model-value="entry.budget_used * 100"
                      :variant="budgetTone(entry.budget_used)"
                      class="h-1.5 w-16"
                      aria-hidden="true"
                    />
                    {{ format.share(entry.budget_used) }}
                  </span>
                  <span
                    v-else
                    class="text-muted"
                  >{{ t('slas.view.noData') }}</span>
                </TableCell>
                <TableCell
                  v-if="hasLatency"
                  kind="numeric"
                  class="max-sm:hidden"
                >
                  <span class="inline-flex items-center justify-end gap-2">
                    <span v-if="entry.latency_ms !== undefined">{{ format.ms(entry.latency_ms) }}</span>
                    <span
                      v-else
                      class="text-muted"
                    >{{ t('slas.view.noData') }}</span>
                    <SlaStandingBadge
                      v-if="settled(entry.latency_outcome)"
                      :standing="entry.latency_outcome"
                    />
                  </span>
                </TableCell>
              </TableRow>
            </TableBody>
          </Table>
        </div>
      </Panel>

      <Panel
        id="sla-incidents"
        :title="t('slas.view.incidents')"
        :meta="String(period.incidents.length)"
        flush
      >
        <div class="overflow-x-auto">
          <Table
            bare
            :label="t('slas.view.incidents')"
          >
            <TableHeader>
              <TableRow>
                <TableHead>{{ t('slas.view.incident') }}</TableHead>
                <TableHead class="max-md:hidden">{{ t('slas.view.impact') }}</TableHead>
                <TableHead>{{ t('slas.view.response') }}</TableHead>
                <TableHead>{{ t('slas.view.resolution') }}</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              <TableEmptyRow
                v-if="period.incidents.length === 0"
                :colspan="4"
                :label="t('slas.view.noIncidents')"
              />
              <TableRow
                v-for="entry in period.incidents"
                :key="entry.incident"
                :data-sla-incident="entry.title"
              >
                <TableCell
                  kind="subject"
                  class="max-w-0 min-w-32 sm:min-w-44"
                >
                  <div class="grid min-w-0 gap-0.5">
                    <RouterLink
                      :to="{ name: 'node-incident', params: { nodeId: routeId, incidentId: entry.incident } }"
                      class="truncate rounded-sm underline-offset-4 hover:underline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring"
                    >
                      {{ entry.title }}
                    </RouterLink>
                    <span class="truncate text-xs font-normal text-muted">{{ dateTime(entry.started_at) }}</span>
                    <ImpactLabel
                      :impact="entry.impact"
                      class="md:hidden"
                    />
                  </div>
                </TableCell>
                <TableCell class="max-md:hidden">
                  <ImpactLabel :impact="entry.impact" />
                </TableCell>
                <TableCell
                  v-for="clock in [entry.response, entry.resolution]"
                  :key="clock === entry.response ? 'response' : 'resolution'"
                >
                  <div class="grid justify-items-start gap-1">
                    <SlaStandingBadge :standing="clock.outcome" />
                    <span
                      v-if="clockText(clock)"
                      class="text-xs text-muted tabular-nums"
                      :title="clock.deadline === undefined ? undefined : t('slas.view.deadline', { time: dateTime(clock.deadline) })"
                    >{{ clockText(clock) }}</span>
                  </div>
                </TableCell>
              </TableRow>
            </TableBody>
          </Table>
        </div>
      </Panel>
    </div>
  </div>
</template>
