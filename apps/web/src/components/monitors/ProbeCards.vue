<script setup lang="ts">
import { useNow } from '@vueuse/core'

import ProbePlace from '~/components/probes/ProbePlace.vue'
import { useDateFormat } from '~/composables/useDateFormat'
import { useFormats } from '~/composables/usePreferences'
import type { Heartbeat, ProbeChoice, ProbeId, ProbeState } from '~/generated'
import { formatLatency, presenceFill, presenceLabels } from '~/lib/monitor-status'
import { useI18n } from '~/plugins/i18n'

/**
 * Each probe's view of a monitor after its own retries, one tile per probe: where it runs, its
 * status and latest checks, and the response time while up or the message otherwise. Choosing
 * a tile shows that probe's own history on the page; choosing it again shows the monitor's.
 * `choices` add the kind and location, `beats` (newest first, any probe) the check strip.
 */
const props = defineProps<{
  states: ProbeState[]
  choices: ProbeChoice[]
  beats: Heartbeat[] | null
  /** The probe whose history the page shows, or `null` for the monitor's. */
  selected: ProbeId | null
  /** The name of a node the caller can read, or `undefined`. */
  nodeName: (id: string) => string | undefined
}>()
const emit = defineEmits<{ select: [probe: ProbeId | null] }>()

const { t, locale } = useI18n()
const { numberLocale } = useFormats()
const { dateTime, relative } = useDateFormat()
const now = useNow({ interval: 10_000 })

const STRIP = 24
const choices = computed(() => new Map(props.choices.map((choice) => [choice.id, choice] as const)))
const strips = computed(() => {
  const map = new Map<ProbeId, Heartbeat[]>()
  for (const beat of props.beats ?? []) {
    if (!beat.probe) continue
    const list = map.get(beat.probe) ?? []
    if (list.length < STRIP) list.push(beat)
    map.set(beat.probe, list)
  }
  return map
})
const location = (probe: ProbeId) => choices.value.get(probe)?.location ?? {}
const strip = (probe: ProbeId) => [...(strips.value.get(probe) ?? [])].reverse()

const kind = (state: ProbeState) => {
  const choice = choices.value.get(state.probe)
  if (!choice) return null
  // The built-in probe is usually named so already.
  if (choice.builtin) return state.name === t('monitors.detail.builtin') ? null : t('monitors.detail.builtin')
  const node = !choice.own && choice.node ? props.nodeName(choice.node) : undefined
  return node ? t('monitors.detail.agentFrom', { node }) : t('monitors.detail.agent')
}
const toggle = (probe: ProbeId) => emit('select', props.selected === probe ? null : probe)
</script>

<template>
  <ul
    class="grid gap-3 @lg:grid-cols-2 @4xl:grid-cols-3"
    :aria-label="t('monitors.detail.probes')"
  >
    <li
      v-for="state in states"
      :key="state.probe"
      :data-probe="state.name"
      :data-live="state.live"
    >
      <button
        type="button"
        class="grid size-full min-w-0 cursor-pointer content-start gap-3 rounded-xl border p-3.5 text-left text-sm transition-colors focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring"
        :class="selected === state.probe ? 'border-accent bg-accent/8' : 'border-hairline bg-surface hover:border-border-strong'"
        :aria-pressed="selected === state.probe"
        :title="t('monitors.history.showProbe', { name: state.name })"
        @click="toggle(state.probe)"
      >
        <div class="flex min-w-0 items-start justify-between gap-3">
          <div class="grid min-w-0 gap-0.5">
            <span class="truncate text-[0.9375rem] font-semibold text-primary">{{ state.name }}</span>
            <ProbePlace
              :location="location(state.probe)"
              class="text-muted"
            />
          </div>
          <span
            class="flex shrink-0 items-center gap-1.5 text-primary"
            :data-status="state.status"
          >
            <span
              class="size-2 rounded-full"
              :class="presenceFill[state.status]"
              aria-hidden="true"
            />
            {{ t(presenceLabels[state.status]) }}
          </span>
        </div>
        <div
          class="flex h-5 items-stretch gap-[2px]"
          aria-hidden="true"
        >
          <span
            v-for="slot in Math.max(0, STRIP - strip(state.probe).length)"
            :key="`empty-${slot}`"
            class="min-w-0 flex-1 rounded-[2px] bg-fill-strong"
          />
          <span
            v-for="beat in strip(state.probe)"
            :key="beat.time"
            class="min-w-0 flex-1 rounded-[2px]"
            :class="presenceFill[beat.status]"
          />
        </div>
        <div class="flex min-w-0 items-baseline justify-between gap-3">
          <span
            v-if="state.status === 'up' && state.latency_ms !== undefined"
            class="text-[1.375rem]/[1.75rem] font-semibold tracking-tight text-primary tabular-nums"
          >{{ formatLatency(state.latency_ms, numberLocale) }}</span>
          <span
            v-else
            class="min-w-0 truncate text-foreground select-text"
            :title="state.message"
          >{{ state.message }}</span>
          <span
            class="shrink-0 text-muted tabular-nums"
            :title="dateTime(state.received_at)"
          >{{ relative(state.received_at, now.getTime(), 'narrow') }}</span>
        </div>
        <p
          v-if="kind(state) || !state.live"
          class="flex flex-wrap gap-x-1.5 text-muted"
        >
          <span v-if="kind(state)">{{ kind(state) }}</span>
          <span
            v-if="!state.live"
            class="text-status-pending"
          >{{ kind(state) ? '· ' : '' }}{{ t('monitors.detail.notInQuorum') }}</span>
        </p>
      </button>
    </li>
  </ul>
</template>
