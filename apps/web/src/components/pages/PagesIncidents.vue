<script setup lang="ts">
import Icon from '~/components/Icon.vue'
import IncidentCard from '~/components/incidents/IncidentCard.vue'
import { Button } from '~/components/ui/button'
import { Skeleton } from '~/components/ui/skeleton'
import { useOpenIncidents } from '~/composables/useIncidents'
import type { NodeId } from '~/generated'
import { useI18n } from '~/plugins/i18n'

/**
 * The open incidents on a node and below, above its status pages, with the way to all of them
 * and, for members who manage incidents, to a new one. Incidents live in observe mode; these
 * links lead there.
 */
const props = defineProps<{ node: NodeId; manage: boolean }>()

const SHOWN = 3
const { t } = useI18n()
const incidents = useOpenIncidents(() => props.node, SHOWN)
const list = computed(() => incidents.data.value?.incidents ?? null)
</script>

<template>
  <section
    class="grid gap-2.5"
    aria-labelledby="pages-incidents-title"
  >
    <div class="flex flex-wrap items-center gap-2">
      <h2
        id="pages-incidents-title"
        class="text-[1.0625rem] font-semibold text-primary"
      >
        {{ t('incidents.open') }}
      </h2>
      <div class="ml-auto flex flex-wrap items-center gap-2">
        <Button
          as-child
          variant="ghost"
          size="sm"
        >
          <RouterLink :to="{ name: 'node-incidents', params: { nodeId: node } }">
            {{ t('incidents.all') }}
            <Icon name="lucide:arrow-right" />
          </RouterLink>
        </Button>
        <Button
          v-if="manage"
          as-child
          variant="fill"
          size="sm"
        >
          <RouterLink :to="{ name: 'node-incident-new', params: { nodeId: node } }">
            <Icon name="lucide:plus" />
            {{ t('incidents.create') }}
          </RouterLink>
        </Button>
      </div>
    </div>
    <Skeleton
      v-if="!list"
      class="h-16 w-full rounded-panel"
    />
    <p
      v-else-if="list.length === 0"
      class="rounded-panel border border-hairline bg-card px-4.5 py-3.5 text-sm text-muted"
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
      >
        <IncidentCard
          :incident="incident"
          :node="incident.node"
        />
      </li>
    </ul>
  </section>
</template>
