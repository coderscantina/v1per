<script setup lang="ts">
import { formatDay, formatUptime } from './format'
import { locale, t } from './messages'
import { toneLabel } from './tones'
import type { BarView } from './view'

/**
 * 90 daily bars with a popover for the day under the pointer or the keyboard. The strip is one
 * tab stop: arrow keys, Home and End move between days, Escape closes the popover. Each bar's
 * label carries everything the popover shows, so the popover itself stays out of the
 * accessibility tree.
 */
const props = defineProps<{ bars: BarView[]; name: string }>()

const strip = useTemplateRef<HTMLElement>('strip')
const buttons = useTemplateRef<HTMLButtonElement[]>('buttons')
/** The day the popover shows, or null. */
const active = ref<number | null>(null)
/** The bar that takes the strip's tab stop: today, until the visitor moves. */
const current = ref(props.bars.length - 1)
const left = ref(0)

const POPOVER_WIDTH = 240

function details(bar: BarView): string[] {
  const lines: string[] = []
  if (bar.maintenance) lines.push(t('bar.maintenance'))
  if (bar.approximate) lines.push(t('bar.approximate'))
  return lines
}

function label(bar: BarView): string {
  const date = formatDay(bar.day, locale.value)
  const status = bar.status === 'none' ? t('bar.noData') : t(toneLabel[bar.status])
  const head =
    bar.uptime === undefined
      ? t('bar.label', { date, status })
      : t('bar.labelUptime', { date, status, uptime: formatUptime(bar.uptime, locale.value) })
  const incidents = bar.incidents.length > 0 ? [`${t('bar.incidents')}: ${bar.incidents.join(', ')}`] : []
  return [head, ...incidents, ...details(bar)].join('. ')
}

const shown = computed(() => (active.value === null ? null : (props.bars[active.value] ?? null)))

/** Places the popover over the bar, kept inside the strip. */
function place(index: number) {
  const box = strip.value?.getBoundingClientRect()
  if (!box) return
  const width = Math.min(POPOVER_WIDTH, box.width)
  const center = ((index + 0.5) / props.bars.length) * box.width
  left.value = Math.min(Math.max(center - width / 2, 0), box.width - width)
}

function show(index: number) {
  active.value = index
  place(index)
}

function move(index: number) {
  const next = Math.min(Math.max(index, 0), props.bars.length - 1)
  current.value = next
  buttons.value?.[next]?.focus()
}

function onKeydown(event: KeyboardEvent) {
  const keys: Record<string, number> = {
    ArrowLeft: current.value - 1,
    ArrowRight: current.value + 1,
    Home: 0,
    End: props.bars.length - 1,
  }
  const target = keys[event.key]
  if (target !== undefined) {
    event.preventDefault()
    move(target)
  } else if (event.key === 'Escape') {
    active.value = null
  }
}

/** The day under the pointer, gaps included. */
function onPointer(event: PointerEvent) {
  const box = strip.value?.getBoundingClientRect()
  if (!box || box.width === 0) return
  const index = Math.floor(((event.clientX - box.left) / box.width) * props.bars.length)
  show(Math.min(Math.max(index, 0), props.bars.length - 1))
}

function onLeave() {
  if (!strip.value?.contains(document.activeElement)) active.value = null
}

function onFocusout(event: FocusEvent) {
  if (!(event.relatedTarget instanceof Node) || !strip.value?.contains(event.relatedTarget)) active.value = null
}
</script>

<template>
  <div class="relative">
    <div
      ref="strip"
      role="group"
      :aria-label="t('components.history', { name })"
      class="flex h-8 items-stretch gap-px sm:gap-[2px]"
      @keydown="onKeydown"
      @pointermove="onPointer"
      @pointerdown="onPointer"
      @pointerleave="onLeave"
      @focusout="onFocusout"
    >
      <button
        v-for="(bar, index) in bars"
        ref="buttons"
        :key="bar.day"
        type="button"
        :data-tone="bar.status"
        :data-day="bar.day"
        :tabindex="index === current ? 0 : -1"
        :aria-label="label(bar)"
        class="min-w-0 flex-1 rounded-[2px] bg-(--tone) transition-opacity"
        :class="active !== null && active !== index ? 'opacity-60' : ''"
        @focus="(current = index), show(index)"
      />
    </div>
    <div
      v-if="shown"
      aria-hidden="true"
      data-testid="bar-popover"
      class="pointer-events-none absolute bottom-full z-10 mb-2 flex flex-col gap-1.5 rounded-lg border border-popover-border bg-popover px-3 py-2.5 text-sm text-popover-foreground shadow-soft-lg"
      :style="{ left: `${left}px`, width: `min(${POPOVER_WIDTH}px, 100%)` }"
    >
      <p class="font-medium text-primary">{{ formatDay(shown.day, locale) }}</p>
      <p
        class="flex items-center gap-2"
        :data-tone="shown.status"
      >
        <span
          class="size-2.5 shrink-0 rounded-full bg-(--tone)"
        />
        <span class="flex-1">{{ shown.status === 'none' ? t('bar.noData') : t(toneLabel[shown.status]) }}</span>
        <span
          v-if="shown.uptime !== undefined"
          class="text-muted"
        >{{ t('bar.uptime', { value: formatUptime(shown.uptime, locale) }) }}</span>
      </p>
      <div
        v-if="shown.incidents.length > 0"
        class="flex flex-col gap-0.5"
      >
        <p class="text-xs font-medium text-muted">{{ t('bar.incidents') }}</p>
        <p
          v-for="(title, index) in shown.incidents"
          :key="index"
        >{{ title }}</p>
      </div>
      <p
        v-for="line in details(shown)"
        :key="line"
        class="text-xs text-muted"
      >{{ line }}</p>
    </div>
  </div>
</template>
