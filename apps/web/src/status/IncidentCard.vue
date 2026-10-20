<script setup lang="ts">
import AffectedList from './AffectedList.vue'
import IncidentMeta from './IncidentMeta.vue'
import { t } from './messages'
import StatusIcon from './StatusIcon.vue'
import { stateLabel, toneIcon, toneLabel } from './tones'
import UpdateTimeline from './UpdateTimeline.vue'
import type { IncidentView } from './view'

/** An open incident: its state and meta line, the affected components and the latest updates. */
const props = defineProps<{ incident: IncidentView; now: number }>()

const LATEST = 3
const expanded = ref(false)
const tone = computed(() => props.incident.impact ?? 'degraded')
const entries = computed(() => (expanded.value ? props.incident.timeline : props.incident.timeline.slice(0, LATEST)))
const timelineId = `timeline-${props.incident.id}`
</script>

<template>
  <article
    :data-incident="incident.title"
    :data-tone="tone"
    class="flex flex-col gap-4 rounded-panel border border-(--tone)/30 bg-(--tone)/8 p-4 text-primary [--muted:color-mix(in_oklab,var(--primary)_72%,transparent)]"
  >
    <header class="flex flex-col gap-1">
      <div class="flex flex-wrap items-start justify-between gap-x-3 gap-y-1">
        <h3 class="min-w-0 flex-1 font-semibold text-primary">{{ incident.title }}</h3>
        <span
          v-if="incident.impact"
          class="inline-flex shrink-0 items-center gap-1.5 text-sm font-medium"
        >
          <StatusIcon
            :name="toneIcon[incident.impact]"
            class="size-4 text-(--tone)"
          />
          {{ t(toneLabel[incident.impact]) }}
        </span>
      </div>
      <IncidentMeta
        :incident="incident"
        :now="now"
      >
        <span
          class="font-medium text-primary"
          data-testid="incident-state"
        >{{ t(stateLabel[incident.state]) }}</span>
        ·
      </IncidentMeta>
    </header>
    <AffectedList :affected="incident.affected" />
    <UpdateTimeline
      :id="timelineId"
      :entries="entries"
    />
    <button
      v-if="incident.timeline.length > LATEST"
      type="button"
      class="inline-flex items-center gap-1 self-start rounded-sm text-sm font-medium text-accent-text"
      :aria-expanded="expanded"
      :aria-controls="timelineId"
      @click="expanded = !expanded"
    >
      <StatusIcon
        name="chevron"
        class="size-4 transition-transform"
        :class="expanded ? 'rotate-180' : ''"
      />
      {{ expanded ? t('incidents.hideTimeline') : t('incidents.timeline') }}
    </button>
  </article>
</template>
