<script setup lang="ts">
import Icon from '~/components/Icon.vue'
import { Panel } from '~/components/ui/panel'
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
  <Panel
    id="node-incidents"
    :title="t('incidents.open')"
    :meta="list ? count : undefined"
    flush
  >
    <template #actions>
      <RouterLink
        :to="{ name: 'node-incidents', params: { nodeId: node } }"
        class="inline-flex items-center gap-1 rounded-sm text-sm text-primary underline-offset-4 hover:underline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring"
      >
        {{ t('incidents.all') }}
        <Icon
          name="lucide:arrow-right"
          size="14"
          aria-hidden="true"
        />
      </RouterLink>
    </template>
    <div class="border-t border-hairline">
      <Skeleton
        v-if="!list"
        class="m-4 h-10"
      />
      <p
        v-else-if="list.length === 0"
        class="px-4.5 py-6 text-sm text-muted"
      >
        {{ t('incidents.noneOpen') }}
      </p>
      <ul
        v-else
        class="divide-y divide-divider"
      >
        <li
          v-for="incident in list"
          :key="incident.id"
          class="flex flex-wrap items-center gap-x-3 gap-y-1 px-4.5 py-2.5"
        >
          <IncidentStateBadge :state="incident.state" />
          <RouterLink
            :to="{ name: 'node-incident', params: { nodeId: incident.node, incidentId: incident.id } }"
            class="min-w-0 flex-1 truncate rounded-sm text-sm font-medium text-primary underline-offset-4 hover:underline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring"
          >
            {{ incident.title }}
          </RouterLink>
          <ImpactLabel
            v-if="incident.impact"
            :impact="incident.impact"
          />
        </li>
      </ul>
    </div>
  </Panel>
</template>
