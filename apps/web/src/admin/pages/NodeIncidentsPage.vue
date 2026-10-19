<script setup lang="ts">
import { useNow } from '@vueuse/core'

import ChoiceGroup from '~/components/ChoiceGroup.vue'
import Icon from '~/components/Icon.vue'
import ImpactLabel from '~/components/incidents/ImpactLabel.vue'
import IncidentStateBadge from '~/components/incidents/IncidentStateBadge.vue'
import LiveIndicator from '~/components/LiveIndicator.vue'
import FilterSelect from '~/components/monitors/FilterSelect.vue'
import PageHeader from '~/components/PageHeader.vue'
import { Alert } from '~/components/ui/alert'
import { Badge } from '~/components/ui/badge'
import { Button } from '~/components/ui/button'
import { EmptyState } from '~/components/ui/empty-state'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableNone, TableRow } from '~/components/ui/table'
import TableEmptyRow from '~/components/ui/TableEmptyRow.vue'
import TableLoadingRow from '~/components/ui/TableLoadingRow.vue'
import { useDateFormat } from '~/composables/useDateFormat'
import { useErrorMessage } from '~/composables/useErrorMessage'
import { useNodeIncidents, type IncidentFilters } from '~/composables/useIncidents'
import { useCurrentNode, useNodeDetail } from '~/composables/useNodes'
import { usePageMeta } from '~/composables/usePageMeta'
import type { IncidentStatusFilter } from '~/generated'
import { can } from '~/lib/access'
import { useI18n } from '~/plugins/i18n'

/**
 * A node's incidents, newest first, live: open ones by default, resolved ones or all on
 * request, manual or automatic. Auto-incidents carry a mark until someone edits them. Filters
 * live in the address, so a link can point at the open auto-incidents.
 */
const { t } = useI18n()
const { message } = useErrorMessage()
const { dateTime, relative } = useDateFormat()
const now = useNow({ interval: 30_000 })
const route = useRoute()
const router = useRouter()
const { routeId } = useCurrentNode()
const detail = useNodeDetail(routeId)
const node = computed(() => detail.data.value?.node ?? null)
const manage = computed(() => can(node.value, 'incidents.manage'))
usePageMeta(() => t('incidents.title'))

type StatusChoice = IncidentStatusFilter | 'all'
const queryText = (key: string) => {
  const value = route.query[key]
  return typeof value === 'string' ? value : ''
}
const status = computed<StatusChoice>({
  get: () => {
    const value = queryText('status')
    return value === 'resolved' || value === 'all' ? value : 'open'
  },
  set: (value) => void router.replace({ query: { ...route.query, status: value === 'open' ? undefined : value } }),
})
const origin = computed({
  get: () => queryText('origin'),
  set: (value: string) => void router.replace({ query: { ...route.query, origin: value || undefined } }),
})

const filters = computed<IncidentFilters>(() => ({
  status: status.value === 'all' ? undefined : status.value,
  origin: origin.value === 'auto' || origin.value === 'manual' ? origin.value : undefined,
}))
const PAGE = 25
const MAX = 100
const limit = ref(PAGE)
watch(filters, () => (limit.value = PAGE))
const incidents = useNodeIncidents(routeId, filters, limit)
const rows = computed(() => incidents.data.value?.incidents ?? [])
const more = computed(() => Boolean(incidents.data.value?.next_cursor))

const statusOptions = computed(() => [
  { value: 'open' as const, label: t('incidents.filters.open') },
  { value: 'resolved' as const, label: t('incidents.filters.resolved') },
  { value: 'all' as const, label: t('incidents.filters.all') },
])
const originOptions = computed(() => [
  { value: 'auto', label: t('incidents.origins.auto') },
  { value: 'manual', label: t('incidents.origins.manual') },
])

const loading = computed(() => !incidents.data.value && !incidents.error.value)
const filtered = computed(() => status.value !== 'open' || origin.value !== '')
const columns = 6
</script>

<template>
  <div>
    <PageHeader
      :title="t('incidents.title')"
      :description="node?.name"
    >
      <template
        v-if="manage && routeId"
        #actions
      >
        <Button
          as-child
          variant="accent"
        >
          <RouterLink :to="{ name: 'node-incident-new', params: { nodeId: routeId } }">
            <Icon name="lucide:plus" />
            {{ t('incidents.create') }}
          </RouterLink>
        </Button>
      </template>
      <template
        v-if="incidents.data.value"
        #freshness
      >
        <LiveIndicator :status="incidents.status.value" />
      </template>
    </PageHeader>

    <Alert
      v-if="detail.error.value && !node"
      color="destructive"
      class="max-w-6xl"
    >
      <p class="text-sm">{{ message(detail.error.value) }}</p>
    </Alert>
    <div
      v-else
      class="grid max-w-6xl gap-4"
    >
      <div class="flex flex-wrap items-center gap-2">
        <ChoiceGroup
          v-model="status"
          :label="t('incidents.filters.status')"
          :options="statusOptions"
        />
        <FilterSelect
          v-model="origin"
          :label="t('incidents.filters.origin')"
          :all-label="t('incidents.filters.anyOrigin')"
          :options="originOptions"
        />
      </div>

      <EmptyState
        v-if="incidents.data.value && rows.length === 0 && !filtered"
        icon="lucide:shield-check"
        :title="t('incidents.empty')"
        :description="manage ? t('incidents.emptyHint') : t('incidents.emptyReadOnly')"
      />

      <Table
        v-else
        :label="t('incidents.title')"
      >
        <TableHeader>
          <TableRow>
            <TableHead class="w-32">{{ t('incidents.columns.state') }}</TableHead>
            <TableHead>{{ t('incidents.columns.title') }}</TableHead>
            <TableHead class="max-sm:hidden">{{ t('incidents.columns.impact') }}</TableHead>
            <TableHead
              numeric
              class="max-md:hidden"
            >
              {{ t('incidents.columns.affected') }}
            </TableHead>
            <TableHead class="max-md:hidden">{{ t('incidents.columns.started') }}</TableHead>
            <TableHead>{{ t('incidents.columns.updated') }}</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          <TableLoadingRow
            v-if="loading"
            :colspan="columns"
            :rows="3"
          />
          <TableEmptyRow
            v-else-if="incidents.error.value && !incidents.data.value"
            :colspan="columns"
            :label="message(incidents.error.value)"
          />
          <TableEmptyRow
            v-else-if="rows.length === 0"
            :colspan="columns"
            :label="t('incidents.noMatch')"
          />
          <TableRow
            v-for="incident in rows"
            :key="incident.id"
            :data-incident="incident.title"
          >
            <TableCell :label="t('incidents.columns.state')">
              <IncidentStateBadge :state="incident.state" />
            </TableCell>
            <TableCell
              kind="subject"
              class="max-w-0 min-w-52"
            >
              <div class="grid min-w-0 gap-1">
                <RouterLink
                  :to="{ name: 'node-incident', params: { nodeId: incident.node, incidentId: incident.id } }"
                  class="truncate rounded-sm underline-offset-4 hover:underline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring"
                >
                  {{ incident.title }}
                </RouterLink>
                <div
                  v-if="incident.origin === 'auto'"
                  class="flex flex-wrap gap-1"
                >
                  <Badge
                    variant="surface"
                    size="sm"
                  >
                    <Icon
                      name="lucide:zap"
                      size="12"
                      class="mr-1"
                      aria-hidden="true"
                    />
                    {{ t('incidents.origins.auto') }}
                  </Badge>
                  <Badge
                    v-if="incident.edited"
                    variant="surface"
                    size="sm"
                  >
                    {{ t('incidents.edited') }}
                  </Badge>
                </div>
              </div>
            </TableCell>
            <TableCell
              class="max-sm:hidden"
              :label="t('incidents.columns.impact')"
            >
              <ImpactLabel
                v-if="incident.impact"
                :impact="incident.impact"
              />
              <TableNone v-else />
            </TableCell>
            <TableCell
              kind="numeric"
              class="max-md:hidden"
            >
              {{ incident.affected }}
            </TableCell>
            <TableCell
              kind="secondary"
              class="whitespace-nowrap max-md:hidden"
            >
              <span :title="dateTime(incident.started_at)">{{ dateTime(incident.started_at) }}</span>
            </TableCell>
            <TableCell
              kind="secondary"
              class="whitespace-nowrap"
              :label="t('incidents.columns.updated')"
            >
              <span :title="dateTime(incident.updated_at)">{{ relative(incident.updated_at, now.getTime()) }}</span>
            </TableCell>
          </TableRow>
        </TableBody>
      </Table>
      <div
        v-if="more"
        class="flex items-center gap-3"
      >
        <Button
          v-if="limit < MAX"
          variant="fill"
          size="sm"
          :loading="incidents.loading.value"
          @click="limit = Math.min(limit + PAGE, MAX)"
        >
          {{ t('incidents.showOlder') }}
        </Button>
        <p
          v-else
          class="text-sm text-muted"
        >
          {{ t('incidents.limit', { count: MAX }) }}
        </p>
      </div>
    </div>
  </div>
</template>
