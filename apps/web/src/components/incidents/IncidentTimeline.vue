<script setup lang="ts">
import { useNow } from '@vueuse/core'

import { useDateFormat } from '~/composables/useDateFormat'
import type { IncidentUpdate } from '~/generated'
import { stateFill, stateLabels, targetKey } from '~/lib/incidents'
import { useI18n } from '~/plugins/i18n'

import ImpactLabel from './ImpactLabel.vue'

/**
 * An incident's updates as posts, newest first, on a vertical line: a marker in the state's
 * colour, the state, who and when, the message and the impacts it set. `names` names targets
 * by key; a target taken out of the incident since is "removed".
 */
const props = defineProps<{ updates: readonly IncidentUpdate[]; names: ReadonlyMap<string, string> }>()

const { t } = useI18n()
const { dateTime, relative } = useDateFormat()
const now = useNow({ interval: 30_000 })
const newest = computed(() => [...props.updates].reverse())
</script>

<template>
  <ol class="grid">
    <li
      v-for="(update, index) in newest"
      :key="update.id"
      class="relative grid gap-1.5 pb-7 pl-8 last:pb-0"
      :data-update="update.state"
    >
      <span
        class="absolute top-1 left-0.5 size-3 rounded-full ring-4 ring-card"
        :class="stateFill[update.state]"
        aria-hidden="true"
      />
      <span
        v-if="index < newest.length - 1"
        class="absolute top-5 bottom-1 left-[0.5625rem] w-px bg-divider"
        aria-hidden="true"
      />
      <div class="flex flex-wrap items-baseline gap-x-2 gap-y-0.5 text-sm">
        <span class="font-semibold text-primary">{{ t(stateLabels[update.state]) }}</span>
        <span class="text-muted">{{ update.author ?? t('incidents.timeline.automatic') }}</span>
        <time
          class="text-muted"
          :datetime="new Date(update.created_at).toISOString()"
          :title="dateTime(update.created_at)"
        >· {{ relative(update.created_at, now.getTime()) }}</time>
      </div>
      <p
        v-if="update.message"
        class="text-[0.9375rem] leading-relaxed whitespace-pre-line text-foreground select-text"
      >
        {{ update.message }}
      </p>
      <ul
        v-if="update.impacts.length > 0"
        class="flex flex-wrap gap-1.5"
      >
        <li
          v-for="entry in update.impacts"
          :key="targetKey(entry.target)"
          class="inline-flex items-center gap-1.5 rounded-md bg-fill px-2 py-0.5 text-sm"
        >
          <span class="text-muted">{{ names.get(targetKey(entry.target)) ?? t('incidents.timeline.removed') }}</span>
          <ImpactLabel :impact="entry.impact" />
        </li>
      </ul>
    </li>
  </ol>
</template>
