<script setup lang="ts">
import { useIntervalFn } from '@vueuse/core'

import Icon from '~/components/Icon.vue'
import FilterSelect from '~/components/monitors/FilterSelect.vue'
import StatusLabel from '~/components/monitors/StatusLabel.vue'
import NodePath from '~/components/nodes/NodePath.vue'
import PageHeader from '~/components/PageHeader.vue'
import SwitchField from '~/components/SwitchField.vue'
import { Alert } from '~/components/ui/alert'
import { Badge } from '~/components/ui/badge'
import { Button } from '~/components/ui/button'
import { EmptyState } from '~/components/ui/empty-state'
import { Input } from '~/components/ui/input'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableMeter, TableNone, TableRow } from '~/components/ui/table'
import TableEmptyRow from '~/components/ui/TableEmptyRow.vue'
import TableLoadingRow from '~/components/ui/TableLoadingRow.vue'
import { useErrorMessage } from '~/composables/useErrorMessage'
import { HISTORY_REFRESH_MS, useNodeMonitors } from '~/composables/useMonitors'
import { useCurrentNode, useNodeDetail } from '~/composables/useNodes'
import { useFormats } from '~/composables/usePreferences'
import { usePageMeta } from '~/composables/usePageMeta'
import type { MonitorType } from '~/generated'
import { can } from '~/lib/access'
import {
  formatLatency,
  formatUptime,
  presenceLabels,
  presenceOf,
  presenceRank,
  presences,
  typeIcons,
  typeLabels,
  type Presence,
} from '~/lib/monitor-status'
import { monitorTarget } from '~/lib/modes'
import { pathTo } from '~/lib/node-tree'
import { useI18n } from '~/plugins/i18n'

/**
 * A node's monitors in one compact list, worst status first, live on the node's topic. Child
 * nodes' monitors join with their path when asked, live on the subtree topic. Status, type and
 * tag filters live in the address, so the overview's counts link straight to a filtered list.
 * A row opens the monitor's editor, staying in configure mode, for those who may change it.
 */
const { t, locale } = useI18n()
const { numberLocale } = useFormats()
const { message } = useErrorMessage()
const route = useRoute()
const router = useRouter()
const { routeId, tree } = useCurrentNode()
const detail = useNodeDetail(routeId)
const node = computed(() => detail.data.value?.node ?? null)
const manage = computed(() => can(node.value, 'monitors.manage'))
usePageMeta(() => t('monitors.title'))


const queryText = (key: string) => {
  const value = route.query[key]
  return typeof value === 'string' ? value : ''
}
const filter = (key: string) =>
  computed({
    get: () => queryText(key),
    set: (value: string) => void router.replace({ query: { ...route.query, [key]: value || undefined } }),
  })
const statusFilter = filter('status')
const typeFilter = filter('type')
const tagFilter = filter('tag')
const childrenFilter = filter('children')
const search = ref('')

const includeChildren = computed({
  get: () => childrenFilter.value === '1',
  set: (value: boolean) => (childrenFilter.value = value ? '1' : ''),
})

const hasChildren = computed(() => Boolean(routeId.value && tree.index.value.children.get(routeId.value)?.length))

const own = useNodeMonitors(routeId, includeChildren)
// Latency and uptime are not pushed; status changes are.
useIntervalFn(() => {
  if (own.data.value && !own.loading.value) void own.reload()
}, HISTORY_REFRESH_MS)

const placed = computed(() => (own.data.value?.monitors ?? []).map((monitor) => ({ monitor, node: monitor.node })))

const fold = (text: string) => text.normalize('NFD').replace(/\p{Diacritic}/gu, '').toLowerCase()

const rows = computed(() => {
  const needle = fold(search.value.trim())
  return placed.value
    .map((entry) => ({ ...entry, presence: presenceOf(entry.monitor) }))
    .filter(({ monitor, presence }) => {
      if (statusFilter.value && presence !== statusFilter.value) return false
      if (typeFilter.value && monitor.type !== typeFilter.value) return false
      if (tagFilter.value && !monitor.tags.includes(tagFilter.value)) return false
      if (!needle) return true
      return [monitor.name, ...monitor.tags].some((text) => fold(text).includes(needle))
    })
    .sort(
      (a, b) =>
        presenceRank(a.presence) - presenceRank(b.presence) || a.monitor.name.localeCompare(b.monitor.name, locale.value)
    )
})

const statusOptions = computed(() =>
  presences.map((presence: Presence) => ({ value: presence, label: t(presenceLabels[presence]) }))
)
const typeOptions = computed(() => {
  const present = new Set<MonitorType>(placed.value.map(({ monitor }) => monitor.type))
  return [...present].map((type) => ({ value: type, label: t(typeLabels[type]) }))
})
const tagOptions = computed(() =>
  [...new Set(placed.value.flatMap(({ monitor }) => monitor.tags))]
    .sort((a, b) => a.localeCompare(b, locale.value))
    .map((tag) => ({ value: tag, label: tag }))
)

const filtered = computed(() => Boolean(statusFilter.value || typeFilter.value || tagFilter.value || search.value.trim()))
const clearFilters = () => {
  search.value = ''
  void router.replace({ query: { ...route.query, status: undefined, type: undefined, tag: undefined } })
}

/** Where a child node's monitor sits, below the node on screen. */
const placeOf = (id: string) => {
  const path = pathTo(tree.index.value, id)
  const at = path.findIndex((entry) => entry.id === routeId.value)
  return path.slice(at + 1)
}

const loading = computed(() => !own.data.value && !own.error.value)
const empty = computed(() => !loading.value && placed.value.length === 0 && !own.error.value)
const columns = 5
</script>

<template>
  <div>
    <PageHeader
      :title="t('monitors.title')"
      :description="node?.name"
    >
      <template
        v-if="manage"
        #actions
      >
        <Button
          as-child
          variant="accent"
        >
          <RouterLink :to="{ name: 'node-monitor-new', params: { nodeId: routeId } }">
            <Icon name="lucide:plus" />
            {{ t('monitors.create') }}
          </RouterLink>
        </Button>
      </template>
    </PageHeader>

    <Alert
      v-if="detail.error.value && !node"
      color="destructive"
      class="max-w-6xl"
    >
      <p class="text-sm">{{ message(detail.error.value) }}</p>
    </Alert>

    <EmptyState
      v-else-if="empty && !includeChildren"
      icon="lucide:activity"
      :title="t('monitors.empty')"
      :description="manage ? t('monitors.emptyHint') : t('monitors.readOnly')"
      class="max-w-6xl"
    >
      <Button
        v-if="manage"
        as-child
        variant="accent"
      >
        <RouterLink :to="{ name: 'node-monitor-new', params: { nodeId: routeId } }">
          <Icon name="lucide:plus" />
          {{ t('monitors.create') }}
        </RouterLink>
      </Button>
      <SwitchField
        v-if="hasChildren"
        id="monitors-children-empty"
        v-model="includeChildren"
        :label="t('monitors.includeChildren')"
      />
    </EmptyState>

    <div
      v-else
      class="grid max-w-6xl gap-4"
    >
      <div class="flex flex-wrap items-center gap-2">
        <div class="relative min-w-0 flex-1 basis-60">
          <Icon
            name="lucide:search"
            class="pointer-events-none absolute top-1/2 left-3 -translate-y-1/2 text-muted"
            aria-hidden="true"
          />
          <Input
            v-model="search"
            type="search"
            class="pl-9"
            :placeholder="t('monitors.search')"
            :aria-label="t('monitors.search')"
          />
        </div>
        <FilterSelect
          v-model="statusFilter"
          :label="t('monitors.filterStatus')"
          :all-label="t('monitors.allStatuses')"
          :options="statusOptions"
        />
        <FilterSelect
          v-model="typeFilter"
          :label="t('monitors.filterType')"
          :all-label="t('monitors.allTypes')"
          :options="typeOptions"
        />
        <FilterSelect
          v-if="tagOptions.length > 0"
          v-model="tagFilter"
          :label="t('monitors.filterTag')"
          :all-label="t('monitors.allTags')"
          :options="tagOptions"
        />
        <SwitchField
          v-if="hasChildren"
          id="monitors-children"
          v-model="includeChildren"
          :label="t('monitors.includeChildren')"
          class="sm:ml-2"
        />
      </div>

      <Table :label="t('monitors.title')">
        <TableHeader>
          <TableRow>
            <TableHead class="w-36 max-sm:hidden">{{ t('monitors.columns.status') }}</TableHead>
            <TableHead>{{ t('monitors.columns.name') }}</TableHead>
            <TableHead class="max-md:hidden">{{ t('monitors.columns.type') }}</TableHead>
            <TableHead
              numeric
              class="max-sm:hidden"
            >
              {{ t('monitors.columns.latency') }}
            </TableHead>
            <TableHead numeric>{{ t('monitors.columns.uptime') }}</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          <TableLoadingRow
            v-if="loading"
            :colspan="columns"
            :rows="4"
          />
          <TableEmptyRow
            v-else-if="own.error.value"
            :colspan="columns"
            :label="message(own.error.value)"
          />
          <TableEmptyRow
            v-else-if="rows.length === 0"
            :colspan="columns"
            :label="filtered ? t('monitors.noMatch') : t('monitors.emptyReadOnly')"
          />
          <TableRow
            v-for="{ monitor, node: at, presence } in rows"
            :key="monitor.id"
            :data-monitor="monitor.name"
          >
            <TableCell
              class="max-sm:hidden"
              :label="t('monitors.columns.status')"
            >
              <StatusLabel :presence="presence" />
            </TableCell>
            <TableCell
              kind="subject"
              class="max-w-0 sm:min-w-56"
            >
              <div class="grid min-w-0 gap-0.5">
                <RouterLink
                  :to="monitorTarget(at, monitor.id, 'configure', manage)"
                  class="truncate rounded-sm text-primary underline-offset-4 hover:underline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring"
                >
                  {{ monitor.name }}
                </RouterLink>
                <StatusLabel
                  :presence="presence"
                  class="sm:hidden"
                />
                <div
                  v-if="at !== routeId || monitor.tags.length > 0"
                  class="flex min-w-0 flex-wrap items-center gap-x-2 gap-y-1 text-xs font-normal text-muted"
                >
                  <NodePath
                    v-if="at !== routeId"
                    :path="placeOf(at)"
                  />
                  <Badge
                    v-for="tag in monitor.tags"
                    :key="tag"
                    variant="surface"
                    size="sm"
                    class="font-medium"
                  >
                    {{ tag }}
                  </Badge>
                </div>
              </div>
            </TableCell>
            <TableCell
              kind="secondary"
              class="whitespace-nowrap max-md:hidden"
            >
              <span class="inline-flex items-center gap-1.5">
                <Icon
                  :name="typeIcons[monitor.type]"
                  size="14"
                  aria-hidden="true"
                />
                {{ t(typeLabels[monitor.type]) }}
              </span>
            </TableCell>
            <TableCell
              kind="numeric"
              class="whitespace-nowrap max-sm:hidden"
              :label="t('monitors.columns.latency')"
            >
              <template v-if="monitor.latency_ms !== undefined">{{ formatLatency(monitor.latency_ms, numberLocale) }}</template>
              <TableNone v-else />
            </TableCell>
            <TableCell
              kind="key"
              class="whitespace-nowrap"
              :label="t('monitors.columns.uptime')"
            >
              <span
                v-if="monitor.uptime_24h !== undefined"
                class="inline-flex items-center justify-end gap-2.5"
              >
                <TableMeter
                  :value="monitor.uptime_24h"
                  class="max-sm:hidden"
                />
                {{ formatUptime(monitor.uptime_24h, numberLocale) }}
              </span>
              <TableNone v-else />
            </TableCell>
          </TableRow>
        </TableBody>
      </Table>
      <Button
        v-if="filtered"
        variant="link"
        size="sm"
        class="justify-self-start"
        @click="clearFilters"
      >
        {{ t('monitors.clearFilters') }}
      </Button>
    </div>
  </div>
</template>
