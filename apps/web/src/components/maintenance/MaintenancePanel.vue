<script setup lang="ts">
import RailSection from '~/components/observe/RailSection.vue'
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
  <RailSection
    id="node-maintenance"
    :title="t('maintenance.title')"
    :meta="relevant ? t('maintenance.activeCount', { count: relevant.active }) : undefined"
    :to="{ name: 'node-maintenance', params: { nodeId: node } }"
    :link-label="t('maintenance.all')"
  >
    <Skeleton
      v-if="!relevant"
      class="h-12 rounded-xl"
    />
    <p
      v-else-if="relevant.list.length === 0"
      class="rounded-xl border border-hairline bg-card px-3 py-2.5 text-[0.8125rem] text-muted"
    >
      {{ t('maintenance.noneUpcoming') }}
    </p>
    <ul
      v-else
      class="grid gap-2"
    >
      <li
        v-for="maintenance in relevant.list"
        :key="maintenance.id"
        class="grid gap-1.5 rounded-xl border border-hairline bg-card px-3 py-2.5"
      >
        <span class="truncate text-[0.8125rem] font-semibold text-primary">{{ maintenance.title }}</span>
        <div class="flex flex-wrap items-center gap-x-3 gap-y-1">
          <MaintenanceStatusLabel :status="maintenance.status" />
          <WindowList
            v-if="maintenance.windows.length > 0"
            :windows="maintenance.windows"
            :timezone="maintenance.schedule.timezone ?? localTimezone()"
            :limit="1"
          />
        </div>
      </li>
    </ul>
  </RailSection>
</template>
