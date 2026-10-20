<script setup lang="ts">
import { formatClock, formatTime, sameDay } from './format'
import { locale, t } from './messages'
import { stateLabel, stateTone, toneLabel } from './tones'
import type { TimelineEntry } from './view'

/**
 * An incident's updates, newest first, on a vertical line with a marker colored by state. The
 * first entry and each new day show the full date, the rest only the time.
 */
const props = defineProps<{ entries: TimelineEntry[] }>()

function when(index: number): string {
  const entry = props.entries[index]
  if (!entry) return ''
  const above = props.entries[index - 1]
  return above && sameDay(above.at, entry.at) ? formatClock(entry.at, locale.value) : formatTime(entry.at, locale.value)
}
</script>

<template>
  <ol
    class="grid"
    :aria-label="t('incidents.updates')"
  >
    <li
      v-for="(entry, index) in entries"
      :key="`${entry.at}-${entry.state}`"
      data-update
      :data-state="entry.state"
      :data-tone="stateTone[entry.state]"
      class="relative grid gap-1 pb-5 pl-7 last:pb-0"
    >
      <span
        v-if="index < entries.length - 1"
        class="absolute top-3 -bottom-3 left-[0.4375rem] w-px bg-border"
        aria-hidden="true"
      />
      <span
        class="absolute top-1.5 left-0.5 size-2.5 rounded-full bg-(--tone) ring-4 ring-card"
        aria-hidden="true"
      />
      <p class="flex flex-wrap items-baseline gap-x-2 text-sm">
        <span class="font-medium text-primary">{{ t(stateLabel[entry.state]) }}</span>
        <time
          class="text-muted"
          :datetime="new Date(entry.at).toISOString()"
        >{{ when(index) }}</time>
      </p>
      <p
        v-if="entry.message"
        class="whitespace-pre-line text-foreground"
      >{{ entry.message }}</p>
      <ul
        v-if="entry.changes.length > 0"
        class="flex flex-wrap gap-x-4 gap-y-1 text-sm"
      >
        <li
          v-for="change in entry.changes"
          :key="change.name"
          :data-change="change.name"
          :data-tone="change.impact ?? 'none'"
          class="inline-flex items-center gap-1.5"
        >
          <span
            class="size-2 shrink-0 rounded-full bg-(--tone)"
            aria-hidden="true"
          />
          <span class="text-primary">{{ change.name }}:</span>
          <span
            v-if="change.impact"
            class="text-muted"
          >{{ t(toneLabel[change.impact]) }}</span>
        </li>
      </ul>
    </li>
  </ol>
</template>
