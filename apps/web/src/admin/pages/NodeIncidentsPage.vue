<script setup lang="ts">
import ChoiceGroup from '~/components/ChoiceGroup.vue'
import Icon from '~/components/Icon.vue'
import IncidentCard from '~/components/incidents/IncidentCard.vue'
import FilterSelect from '~/components/monitors/FilterSelect.vue'
import PageHeader from '~/components/PageHeader.vue'
import { Alert } from '~/components/ui/alert'
import { Button } from '~/components/ui/button'
import { EmptyState } from '~/components/ui/empty-state'
import { Skeleton } from '~/components/ui/skeleton'
import { useErrorMessage } from '~/composables/useErrorMessage'
import { useNodeIncidents, type IncidentFilters } from '~/composables/useIncidents'
import { useCurrentNode, useNodeDetail } from '~/composables/useNodes'
import { usePageMeta } from '~/composables/usePageMeta'
import type { IncidentStatusFilter } from '~/generated'
import { can } from '~/lib/access'
import { openFirst } from '~/lib/incidents'
import { useI18n } from '~/plugins/i18n'

/**
 * A node's incidents and those below it as a feed of cards, open ones first, then newest first,
 * live: open ones by default, resolved ones or all on request, manual or automatic.
 * Auto-incidents carry a mark until someone edits them. Filters live in the address, so a link
 * can point at the open auto-incidents.
 */
const { t } = useI18n()
const { message } = useErrorMessage()
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
  subtree: true,
  status: status.value === 'all' ? undefined : status.value,
  origin: origin.value === 'auto' || origin.value === 'manual' ? origin.value : undefined,
}))
const PAGE = 25
const MAX = 100
const limit = ref(PAGE)
watch(filters, () => (limit.value = PAGE))
const incidents = useNodeIncidents(routeId, filters, limit)
const rows = computed(() => openFirst(incidents.data.value?.incidents ?? []))
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
          size="lg"
        >
          <RouterLink :to="{ name: 'node-incident-new', params: { nodeId: routeId } }">
            <Icon name="lucide:plus" />
            {{ t('incidents.create') }}
          </RouterLink>
        </Button>
      </template>
    </PageHeader>

    <Alert
      v-if="detail.error.value && !node"
      color="destructive"
      class="max-w-3xl"
    >
      <p class="text-sm">{{ message(detail.error.value) }}</p>
    </Alert>
    <div
      v-else
      class="grid max-w-3xl gap-4"
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

      <div
        v-if="loading"
        class="grid gap-2.5"
      >
        <Skeleton
          v-for="index in 3"
          :key="index"
          class="h-20 w-full rounded-panel"
        />
      </div>
      <Alert
        v-else-if="incidents.error.value && !incidents.data.value"
        color="destructive"
      >
        <p class="text-sm">{{ message(incidents.error.value) }}</p>
      </Alert>
      <EmptyState
        v-else-if="rows.length === 0 && !filtered"
        icon="lucide:shield-check"
        :title="t('incidents.empty')"
        :description="manage ? undefined : t('incidents.emptyReadOnly')"
      >
        <Button
          v-if="manage && routeId"
          as-child
          variant="accent"
        >
          <RouterLink :to="{ name: 'node-incident-new', params: { nodeId: routeId } }">
            <Icon name="lucide:plus" />
            {{ t('incidents.create') }}
          </RouterLink>
        </Button>
      </EmptyState>
      <p
        v-else-if="rows.length === 0"
        class="rounded-panel border border-hairline bg-card px-4.5 py-6 text-center text-sm text-muted"
      >
        {{ t('incidents.noMatch') }}
      </p>
      <ul
        v-else
        class="grid gap-2.5"
        :aria-label="t('incidents.title')"
      >
        <li
          v-for="incident in rows"
          :key="incident.id"
        >
          <IncidentCard
            :incident="incident"
            :node="incident.node"
          />
        </li>
      </ul>

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
