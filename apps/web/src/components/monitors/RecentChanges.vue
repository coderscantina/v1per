<script setup lang="ts">
import { useNow } from '@vueuse/core'

import RailSection from '~/components/observe/RailSection.vue'
import { Skeleton } from '~/components/ui/skeleton'
import { useDateFormat } from '~/composables/useDateFormat'
import type { Heartbeat, ProbeId } from '~/generated'
import { presenceFill, presenceLabels } from '~/lib/monitor-status'
import { useI18n } from '~/plugins/i18n'

import { formatSpan, statusChanges, type StatusChange } from './monitor-detail'

/**
 * Status changes among the latest checks, newest first: a recovery says how long the outage
 * lasted, any other change its message. With `probes` (names by ID), each names its probe.
 */
const props = defineProps<{ beats: Heartbeat[] | null; probes?: Map<ProbeId, string> | null }>()

const SHOWN = 6
const { t, locale } = useI18n()
const { dateTime, relative } = useDateFormat()
const now = useNow({ interval: 10_000 })

const changes = computed(() => (props.beats ? statusChanges(props.beats).slice(0, SHOWN) : null))

const label = ({ beat, after }: StatusChange) => {
  const text =
    beat.status === 'up'
      ? after === undefined
        ? t(presenceLabels.up)
        : t('monitors.detail.recovered', { span: formatSpan(after, locale.value) })
      : beat.message || t(presenceLabels[beat.status])
  const probe = beat.probe ? props.probes?.get(beat.probe) : undefined
  return probe ? `${probe}: ${text}` : text
}
</script>

<template>
  <RailSection
    id="monitor-changes"
    :title="t('monitors.detail.changes')"
  >
    <Skeleton
      v-if="!changes"
      class="h-16 rounded-control"
    />
    <p
      v-else-if="changes.length === 0"
      class="text-[0.8125rem] text-muted"
    >
      {{ t('monitors.detail.changesNone') }}
    </p>
    <ul
      v-else
      class="divide-y divide-hairline"
    >
      <li
        v-for="change in changes"
        :key="`${change.beat.time}-${change.beat.probe ?? ''}`"
        class="flex items-start gap-2.5 py-2 first:pt-0"
      >
        <span
          class="mt-1.5 size-2 shrink-0 rounded-full"
          :class="presenceFill[change.beat.status]"
          aria-hidden="true"
        />
        <span class="sr-only">{{ t(presenceLabels[change.beat.status]) }}:</span>
        <span class="min-w-0 flex-1 text-[0.8125rem] break-words text-foreground">{{ label(change) }}</span>
        <span
          class="shrink-0 text-[0.8125rem] text-muted tabular-nums"
          :title="dateTime(change.beat.time)"
        >{{ relative(change.beat.time, now.getTime(), 'narrow') }}</span>
      </li>
    </ul>
  </RailSection>
</template>
