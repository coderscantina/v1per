<script setup lang="ts">
import type { SlaPeriodKind } from '~/generated'

import { locale, t } from './messages'
import { formatPeriod, periodLines, type SlaPeriodView } from './sla'
import { slaLabel, slaTone } from './tones'

/**
 * Up to 12 closed periods as a row of slots, newest on the right, with a popover for the period
 * under the pointer or the keyboard. Like the daily bars, the row is one tab stop: arrow keys,
 * Home and End move between periods, Escape closes the popover, and each slot's label carries
 * what the popover shows.
 */
const props = defineProps<{ periods: SlaPeriodView[]; kind: SlaPeriodKind; name: string }>()

const SLOTS = 12
const POPOVER_WIDTH = 260

const strip = useTemplateRef<HTMLElement>('strip')
const active = ref<number | null>(null)
/** The slot that takes the row's tab stop: the newest, until the visitor moves. */
const chosen = ref<number | null>(null)
const current = computed(() => Math.min(chosen.value ?? props.periods.length - 1, props.periods.length - 1))
const left = ref(0)

const slots = computed(() =>
  props.periods.map((period) => {
    const title = formatPeriod(period.start, props.kind, locale.value)
    const lines = periodLines(period, locale.value)
    const head = t('sla.periodLabel', { period: title, state: t(slaLabel[period.state]) })
    return { period, title, lines, label: [head, ...lines.map((line) => line.text)].join('. ') }
  })
)
const shown = computed(() => (active.value === null ? null : (slots.value[active.value] ?? null)))
/** Fewer than 12 periods start further right, so the newest always sits at the end. */
const offset = computed(() => SLOTS - Math.min(props.periods.length, SLOTS) + 1)
/** The axis names the oldest period under its slot once there is room for both labels. */
const axis = computed(() => ({
  oldest: props.periods.length >= 4 ? slots.value[0]?.period.start : undefined,
  newest: slots.value.at(-1)?.period.start,
}))

function show(index: number) {
  active.value = index
  const box = strip.value?.getBoundingClientRect()
  if (!box) return
  const width = Math.min(POPOVER_WIDTH, box.width)
  const center = ((offset.value - 1 + index + 0.5) / SLOTS) * box.width
  left.value = Math.min(Math.max(center - width / 2, 0), box.width - width)
}

function move(index: number) {
  const next = Math.min(Math.max(index, 0), props.periods.length - 1)
  chosen.value = next
  // In slot order: a v-for ref array keeps no order once the list re-renders.
  strip.value?.querySelectorAll('button')[next]?.focus()
}

function onKeydown(event: KeyboardEvent) {
  const keys: Record<string, number> = {
    ArrowLeft: current.value - 1,
    ArrowRight: current.value + 1,
    Home: 0,
    End: props.periods.length - 1,
  }
  const target = keys[event.key]
  if (target !== undefined) {
    event.preventDefault()
    move(target)
  } else if (event.key === 'Escape') {
    active.value = null
  }
}

function onLeave() {
  if (!strip.value?.contains(document.activeElement)) active.value = null
}

function onFocusout(event: FocusEvent) {
  if (!(event.relatedTarget instanceof Node) || !strip.value?.contains(event.relatedTarget)) active.value = null
}
</script>

<template>
  <div class="relative flex flex-col gap-1.5">
    <div
      ref="strip"
      role="group"
      :aria-label="t('sla.historyLabel', { name })"
      class="grid h-7 grid-cols-12 gap-[3px]"
      @keydown="onKeydown"
      @pointerleave="onLeave"
      @focusout="onFocusout"
    >
      <button
        v-for="(slot, index) in slots"
        :key="slot.period.start"
        type="button"
        :data-tone="slaTone[slot.period.state]"
        :data-period="slot.period.start"
        :tabindex="index === current ? 0 : -1"
        :aria-label="slot.label"
        :style="index === 0 ? { gridColumnStart: offset } : undefined"
        class="min-w-0 rounded-[3px] bg-(--tone) transition-opacity"
        :class="active !== null && active !== index ? 'opacity-60' : ''"
        @focus="(chosen = index), show(index)"
        @pointerenter="show(index)"
        @pointerdown="show(index)"
      />
    </div>
    <p
      aria-hidden="true"
      class="relative flex h-4 justify-end text-[13px] leading-4 text-muted"
    >
      <span
        v-if="axis.oldest"
        class="absolute"
        :style="{ left: `${((offset - 1) / SLOTS) * 100}%` }"
      >{{ formatPeriod(axis.oldest, kind, locale, true) }}</span>
      <span v-if="axis.newest">{{ formatPeriod(axis.newest, kind, locale, true) }}</span>
    </p>
    <div
      v-if="shown"
      aria-hidden="true"
      data-testid="sla-popover"
      class="pointer-events-none absolute bottom-full z-10 mb-2 flex flex-col gap-1.5 rounded-lg border border-popover-border bg-popover px-3 py-2.5 text-sm text-popover-foreground shadow-soft-lg"
      :style="{ left: `${left}px`, width: `min(${POPOVER_WIDTH}px, 100%)` }"
    >
      <p
        class="flex items-center gap-2"
        :data-tone="slaTone[shown.period.state]"
      >
        <span class="flex-1 font-medium text-primary">{{ shown.title }}</span>
        <span class="size-2.5 shrink-0 rounded-full bg-(--tone)" />
        <span>{{ t(slaLabel[shown.period.state]) }}</span>
      </p>
      <p
        v-for="line in shown.lines"
        :key="line.text"
        class="flex items-start gap-2 text-[13px]"
        :data-tone="slaTone[line.state]"
      >
        <span class="mt-1.5 size-2 shrink-0 rounded-full bg-(--tone)" />
        <span>{{ line.text }}</span>
      </p>
    </div>
  </div>
</template>
