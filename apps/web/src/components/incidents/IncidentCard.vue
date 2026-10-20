<script setup lang="ts">
import { useNow } from '@vueuse/core'

import Icon from '~/components/Icon.vue'
import { Badge } from '~/components/ui/badge'
import { useDateFormat } from '~/composables/useDateFormat'
import type { Incident, NodeId } from '~/generated'
import { stateFill } from '~/lib/incidents'
import { useI18n } from '~/plugins/i18n'

import ImpactLabel from './ImpactLabel.vue'
import IncidentStateBadge from './IncidentStateBadge.vue'

/**
 * One incident in a feed: its state, title, impact and when it started, linking to its page.
 * Takes a node's incident or one a status page shows, which carries less; `node` scopes the
 * link, and the incident page moves to the incident's own node if that differs.
 */
const props = defineProps<{
  incident: Pick<Incident, 'id' | 'title' | 'state' | 'impact' | 'started_at'> &
    Partial<Pick<Incident, 'origin' | 'edited' | 'affected' | 'updated_at'>>
  node: NodeId
  /** The latest message, where the source has it. */
  latest?: string
}>()

const { t } = useI18n()
const { dateTime, relative } = useDateFormat()
const now = useNow({ interval: 30_000 })
const to = computed(() => ({ name: 'node-incident', params: { nodeId: props.node, incidentId: props.incident.id } }))
</script>

<template>
  <article
    class="relative flex gap-3 rounded-panel border border-hairline bg-card px-4.5 py-3.5 transition-colors duration-200 ease-butter hover:border-border-strong max-sm:px-4"
    :data-incident="incident.title"
  >
    <span
      class="mt-1.5 size-2.5 shrink-0 rounded-full"
      :class="stateFill[incident.state]"
      aria-hidden="true"
    />
    <div class="grid min-w-0 flex-1 gap-1.5">
      <div class="flex flex-wrap items-center gap-x-2.5 gap-y-1">
        <RouterLink
          :to="to"
          class="min-w-0 truncate rounded-sm font-semibold text-primary after:absolute after:inset-0 after:rounded-panel focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring"
        >
          {{ incident.title }}
        </RouterLink>
        <Badge
          v-if="incident.origin === 'auto'"
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
      <p
        v-if="latest"
        class="line-clamp-2 text-sm text-foreground"
      >
        {{ latest }}
      </p>
      <div class="flex flex-wrap items-center gap-x-3 gap-y-1 text-sm text-muted">
        <IncidentStateBadge :state="incident.state" />
        <ImpactLabel
          v-if="incident.impact && incident.state !== 'resolved'"
          :impact="incident.impact"
        />
        <span :title="dateTime(incident.started_at)">{{ t('incidents.feed.started', { time: relative(incident.started_at, now.getTime()) }) }}</span>
        <span v-if="incident.affected !== undefined">{{ t('incidents.feed.affected', { count: incident.affected }, incident.affected) }}</span>
        <span
          v-if="incident.updated_at !== undefined && incident.updated_at !== incident.started_at"
          :title="dateTime(incident.updated_at)"
        >{{ t('incidents.feed.updated', { time: relative(incident.updated_at, now.getTime()) }) }}</span>
      </div>
    </div>
  </article>
</template>
