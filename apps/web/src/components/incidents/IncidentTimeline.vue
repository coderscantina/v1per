<script setup lang="ts">
import { useDateFormat } from '~/composables/useDateFormat'
import type { IncidentUpdate } from '~/generated'
import { targetKey } from '~/lib/incidents'
import { useI18n } from '~/plugins/i18n'

import ImpactLabel from './ImpactLabel.vue'
import IncidentStateBadge from './IncidentStateBadge.vue'

/**
 * An incident's updates, newest first: state, when and by whom, the message and the impacts it
 * set. `names` names targets by key; a target taken out of the incident since is "removed".
 */
const props = defineProps<{ updates: readonly IncidentUpdate[]; names: ReadonlyMap<string, string> }>()

const { t } = useI18n()
const { dateTime } = useDateFormat()
const newest = computed(() => [...props.updates].reverse())
</script>

<template>
  <ol class="grid">
    <li
      v-for="(update, index) in newest"
      :key="update.id"
      class="relative grid gap-2 pb-6 pl-7 last:pb-0"
      :data-update="update.state"
    >
      <span
        class="absolute top-1.5 left-1 size-2.5 rounded-full ring-4 ring-card"
        :class="index === 0 ? 'bg-primary' : 'bg-fill-strong'"
        aria-hidden="true"
      />
      <span
        v-if="index < newest.length - 1"
        class="absolute top-5 bottom-0 left-[0.5625rem] w-px bg-divider"
        aria-hidden="true"
      />
      <div class="flex flex-wrap items-center gap-x-3 gap-y-1">
        <IncidentStateBadge :state="update.state" />
        <time
          class="text-sm text-muted"
          :datetime="new Date(update.created_at).toISOString()"
        >{{ dateTime(update.created_at) }}</time>
        <span class="text-sm text-muted">· {{ update.author ?? t('incidents.timeline.automatic') }}</span>
      </div>
      <p
        v-if="update.message"
        class="text-sm whitespace-pre-line text-foreground"
      >
        {{ update.message }}
      </p>
      <ul
        v-if="update.impacts.length > 0"
        class="flex flex-wrap gap-x-4 gap-y-1"
      >
        <li
          v-for="entry in update.impacts"
          :key="targetKey(entry.target)"
          class="inline-flex items-center gap-1.5 text-sm"
        >
          <span class="text-muted">{{ names.get(targetKey(entry.target)) ?? t('incidents.timeline.removed') }}:</span>
          <ImpactLabel :impact="entry.impact" />
        </li>
      </ul>
    </li>
  </ol>
</template>
