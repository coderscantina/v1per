<script setup lang="ts">
import { historyRanges, type HistorySlot, type HistoryWindow } from '~/lib/monitor-history'
import { barFill, barTone, formatUptime } from '~/lib/monitor-status'
import { useI18n } from '~/plugins/i18n'
import { useDateFormat } from '~/composables/useDateFormat'
import { useFormats } from '~/composables/usePreferences'

/**
 * Uptime of a time frame as bars, oldest first, each coloured by its slot's uptime. Hovering a
 * bar reads out its time, uptime and failed checks above the bars; assistive technology gets a
 * summary.
 */
const props = defineProps<{ slots: HistorySlot[]; window: HistoryWindow }>()

const { t, locale } = useI18n()
const { numberLocale } = useFormats()
const { date, dateTime, time } = useDateFormat()

const daily = computed(() => historyRanges[props.window.range].slot >= 86_400_000)
const when = (slot: HistorySlot) =>
  daily.value
    ? date(slot.start, 'UTC')
    : `${dateTime(slot.start)} – ${time(slot.end)}`

const hovered = ref<HistorySlot | null>(null)
const readout = (slot: HistorySlot) => {
  const uptime = formatUptime(slot.uptime, numberLocale.value)
  if (!uptime) return slot.maintenance > 0 ? t('monitors.history.maintenanceOnly') : t('monitors.detail.noData')
  return slot.down > 0 ? t('monitors.history.slotDown', { uptime, count: slot.down }) : uptime
}
const summary = computed(() => {
  const down = props.slots.filter((slot) => slot.down > 0).length
  return t('monitors.history.summary', { down, count: props.slots.length })
})
</script>

<template>
  <figure class="m-0 grid gap-1.5">
    <figcaption class="sr-only">{{ summary }}</figcaption>
    <p
      class="min-h-5 truncate text-[0.8125rem] tabular-nums"
      aria-hidden="true"
    >
      <template v-if="hovered">
        <span class="text-primary">{{ when(hovered) }}</span>
        <span class="text-muted"> · {{ readout(hovered) }}</span>
      </template>
      <span
        v-else
        class="text-muted"
      >{{ summary }}</span>
    </p>
    <div
      class="flex h-9 items-stretch gap-px sm:gap-[2px]"
      aria-hidden="true"
      @pointerleave="hovered = null"
    >
      <span
        v-for="slot in slots"
        :key="slot.start"
        class="min-w-0 flex-1 rounded-[2px] transition-opacity"
        :class="[barFill[barTone(slot)], hovered && hovered !== slot && 'opacity-50']"
        :data-tone="barTone(slot)"
        @pointerenter="hovered = slot"
      />
    </div>
  </figure>
</template>
