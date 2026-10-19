<script setup lang="ts">
import AffectedList from './AffectedList.vue'
import { formatTime } from './format'
import { locale, t } from './messages'
import StatusIcon from './StatusIcon.vue'
import { stateLabel, toneIcon, toneLabel } from './tones'
import UpdateTimeline from './UpdateTimeline.vue'
import type { IncidentView } from './view'

/** An open incident: its latest update up front, the rest of the timeline on request. */
const props = defineProps<{ incident: IncidentView }>()

const expanded = ref(false)
const tone = computed(() => props.incident.impact ?? 'degraded')
const timelineId = `timeline-${props.incident.id}`
</script>

<template>
  <article
    :data-incident="incident.title"
    :data-tone="tone"
    class="flex flex-col gap-3 rounded-panel border border-border bg-card p-4 shadow-soft-sm"
  >
    <header class="flex flex-wrap items-start justify-between gap-x-3 gap-y-1">
      <h3 class="min-w-0 flex-1 font-semibold text-primary">{{ incident.title }}</h3>
      <span
        v-if="incident.impact"
        class="inline-flex shrink-0 items-center gap-1.5 text-sm text-foreground"
      >
        <StatusIcon
          :name="toneIcon[incident.impact]"
          class="size-4 text-(--tone)"
        />
        {{ t(toneLabel[incident.impact]) }}
      </span>
    </header>
    <div
      v-if="incident.latest"
      class="flex flex-col gap-1"
    >
      <p class="text-sm text-muted">
        <span
          class="font-medium text-primary"
          data-testid="incident-state"
        >{{ t(stateLabel[incident.latest.state]) }}</span>
        · <time :datetime="new Date(incident.latest.created_at).toISOString()">{{ formatTime(incident.latest.created_at, locale) }}</time>
      </p>
      <p class="whitespace-pre-line text-foreground">{{ incident.latest.message }}</p>
    </div>
    <AffectedList :affected="incident.affected" />
    <template v-if="incident.updates.length > 1">
      <button
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
      <UpdateTimeline
        v-show="expanded"
        :id="timelineId"
        :updates="incident.updates.slice(1)"
      />
    </template>
  </article>
</template>
