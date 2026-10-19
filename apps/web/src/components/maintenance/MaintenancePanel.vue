<script setup lang="ts">
import Icon from '~/components/Icon.vue'
import { Panel } from '~/components/ui/panel'
import { Skeleton } from '~/components/ui/skeleton'
import { useMaintenanceSummary } from '~/composables/useMaintenance'
import { localTimezone } from '~/composables/useMonitors'
import type { NodeId } from '~/generated'
import { useI18n } from '~/plugins/i18n'

import MaintenanceStatusLabel from './MaintenanceStatusLabel.vue'
import WindowList from './WindowList.vue'

/** Maintenance running now on a node and below, then the next planned windows, read every 30 s. */
const props = defineProps<{ node: NodeId }>()

const SHOWN = 4
const { t } = useI18n()
const live = useMaintenanceSummary(() => props.node)
const relevant = computed(() => {
  const all = live.data.value?.maintenances
  if (!all) return null
  const active = all.filter((entry) => entry.status === 'under_maintenance')
  const next = all
    .filter((entry) => entry.status === 'scheduled')
    .sort((a, b) => (a.windows[0]?.start ?? Infinity) - (b.windows[0]?.start ?? Infinity))
  return { active: active.length, list: [...active, ...next].slice(0, SHOWN) }
})
</script>

<template>
  <Panel
    id="node-maintenance"
    :title="t('maintenance.title')"
    :meta="relevant ? t('maintenance.activeCount', { count: relevant.active }) : undefined"
    flush
  >
    <template #actions>
      <RouterLink
        :to="{ name: 'node-maintenance', params: { nodeId: node } }"
        class="inline-flex items-center gap-1 rounded-sm text-sm text-primary underline-offset-4 hover:underline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring"
      >
        {{ t('maintenance.all') }}
        <Icon
          name="lucide:arrow-right"
          size="14"
          aria-hidden="true"
        />
      </RouterLink>
    </template>
    <div class="border-t border-hairline">
      <Skeleton
        v-if="!relevant"
        class="m-4 h-10"
      />
      <p
        v-else-if="relevant.list.length === 0"
        class="px-4.5 py-6 text-sm text-muted"
      >
        {{ t('maintenance.noneUpcoming') }}
      </p>
      <ul
        v-else
        class="divide-y divide-divider"
      >
        <li
          v-for="maintenance in relevant.list"
          :key="maintenance.id"
          class="grid gap-1 px-4.5 py-2.5 sm:grid-cols-[9rem_minmax(0,1fr)_auto] sm:items-center sm:gap-3"
        >
          <MaintenanceStatusLabel :status="maintenance.status" />
          <span class="truncate text-sm font-medium text-primary">{{ maintenance.title }}</span>
          <WindowList
            v-if="maintenance.windows.length > 0"
            :windows="maintenance.windows"
            :timezone="maintenance.schedule.timezone ?? localTimezone()"
            :limit="1"
          />
        </li>
      </ul>
    </div>
  </Panel>
</template>
