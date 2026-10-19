<script setup lang="ts">
import RailSection from '~/components/observe/RailSection.vue'
import { Skeleton } from '~/components/ui/skeleton'
import { useOpenIncidents } from '~/composables/useIncidents'
import type { NodeId } from '~/generated'
import { useI18n } from '~/plugins/i18n'

import ImpactLabel from './ImpactLabel.vue'
import IncidentStateBadge from './IncidentStateBadge.vue'

/** The open incidents on a node and below, newest first, live; each links to its page. */
const props = defineProps<{ node: NodeId }>()

const SHOWN = 5
const { t } = useI18n()
const incidents = useOpenIncidents(() => props.node, SHOWN)
const list = computed(() => incidents.data.value?.incidents ?? null)
const count = computed(() => {
  const shown = list.value?.length ?? 0
  return incidents.data.value?.next_cursor ? `${shown}+` : String(shown)
})
</script>

<template>
  <RailSection
    id="node-incidents"
    :title="t('incidents.open')"
    :meta="list ? count : undefined"
    :to="{ name: 'node-incidents', params: { nodeId: node } }"
    :link-label="t('incidents.all')"
  >
    <Skeleton
      v-if="!list"
      class="h-12 rounded-xl"
    />
    <p
      v-else-if="list.length === 0"
      class="rounded-xl border border-hairline bg-card px-3 py-2.5 text-[0.8125rem] text-muted"
    >
      {{ t('incidents.noneOpen') }}
    </p>
    <ul
      v-else
      class="grid gap-2"
    >
      <li
        v-for="incident in list"
        :key="incident.id"
        class="grid gap-1.5 rounded-xl border border-hairline bg-card px-3 py-2.5"
      >
        <RouterLink
          :to="{ name: 'node-incident', params: { nodeId: incident.node, incidentId: incident.id } }"
          class="truncate rounded-sm text-[0.8125rem] font-semibold text-primary underline-offset-4 hover:underline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring"
        >
          {{ incident.title }}
        </RouterLink>
        <div class="flex flex-wrap items-center gap-x-3 gap-y-1">
          <IncidentStateBadge :state="incident.state" />
          <ImpactLabel
            v-if="incident.impact"
            :impact="incident.impact"
          />
        </div>
      </li>
    </ul>
  </RailSection>
</template>
