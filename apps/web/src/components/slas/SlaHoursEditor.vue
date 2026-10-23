<script setup lang="ts">
import Icon from '~/components/Icon.vue'
import { Button } from '~/components/ui/button'
import { Input } from '~/components/ui/input'
import { useFormats } from '~/composables/usePreferences'
import { weekStart } from '~/lib/formats'
import { nextRange, officeHours, slaWeekdays, type HoursRange } from '~/lib/sla-form'
import { useI18n } from '~/plugins/i18n'

/**
 * Weekly business hours: one row per weekday from the reader's first day of the week, each
 * with any number of ranges. A day without ranges is closed; a week without any runs around
 * the clock. `errors` holds a problem per weekday, Sunday first.
 */
const props = defineProps<{ errors: readonly (string | null)[] }>()
const model = defineModel<HoursRange[][]>({ required: true })

const { t, locale } = useI18n()
const { formats } = useFormats()

/** 7 January 2024 was a Sunday. */
const days = computed(() => {
  const format = new Intl.DateTimeFormat(locale.value, { weekday: 'long', timeZone: 'UTC' })
  const first = weekStart(formats.value)
  return [...slaWeekdays]
    .sort((a, b) => ((a - first + 7) % 7) - ((b - first + 7) % 7))
    .map((weekday) => ({ weekday, name: format.format(Date.UTC(2024, 0, 7 + weekday)) }))
})
const empty = computed(() => model.value.every((ranges) => ranges.length === 0))

const change = (weekday: number, ranges: HoursRange[]) => {
  model.value = model.value.map((current, index) => (index === weekday ? ranges : current))
}
const add = (weekday: number) => {
  const ranges = model.value[weekday] ?? []
  change(weekday, [...ranges, nextRange(ranges)])
}
const remove = (weekday: number, at: number) => change(weekday, (model.value[weekday] ?? []).filter((_, index) => index !== at))
const set = (weekday: number, at: number, part: keyof HoursRange, value: string | number) =>
  change(
    weekday,
    (model.value[weekday] ?? []).map((range, index) => (index === at ? { ...range, [part]: String(value) } : range))
  )
</script>

<template>
  <div class="grid gap-3">
    <div class="divide-y divide-divider rounded-control border border-hairline">
      <div
        v-for="day in days"
        :key="day.weekday"
        class="grid gap-2 px-3 py-2 sm:grid-cols-[8rem_minmax(0,1fr)] sm:items-start"
        :data-weekday="day.weekday"
      >
        <span class="text-sm font-medium text-primary sm:pt-2">{{ day.name }}</span>
        <div class="grid gap-2">
          <div
            v-for="(range, index) in model[day.weekday]"
            :key="index"
            class="flex items-center gap-2"
          >
            <Input
              type="time"
              class="w-28"
              :model-value="range.start"
              :aria-label="t('slas.editor.rangeStart', { day: day.name })"
              :aria-invalid="props.errors[day.weekday] ? 'true' : undefined"
              @update:model-value="(value) => set(day.weekday, index, 'start', value)"
            />
            <span class="text-sm text-muted">{{ t('slas.editor.to') }}</span>
            <Input
              type="time"
              class="w-28"
              :model-value="range.end"
              :aria-label="t('slas.editor.rangeEnd', { day: day.name })"
              :aria-invalid="props.errors[day.weekday] ? 'true' : undefined"
              @update:model-value="(value) => set(day.weekday, index, 'end', value)"
            />
            <Button
              type="button"
              variant="ghost"
              size="icon"
              :aria-label="t('slas.editor.removeRange', { day: day.name, start: range.start, end: range.end })"
              @click="remove(day.weekday, index)"
            >
              <Icon name="lucide:x" />
            </Button>
            <Button
              v-if="index === (model[day.weekday] ?? []).length - 1"
              type="button"
              variant="ghost"
              size="icon"
              :aria-label="t('slas.editor.addRange', { day: day.name })"
              @click="add(day.weekday)"
            >
              <Icon name="lucide:plus" />
            </Button>
          </div>
          <div
            v-if="(model[day.weekday] ?? []).length === 0"
            class="flex h-9 items-center gap-2"
          >
            <span class="text-sm text-muted">{{ t('slas.editor.closed') }}</span>
            <Button
              type="button"
              variant="ghost"
              size="icon"
              :aria-label="t('slas.editor.addRange', { day: day.name })"
              @click="add(day.weekday)"
            >
              <Icon name="lucide:plus" />
            </Button>
          </div>
          <p
            v-if="props.errors[day.weekday]"
            class="text-sm font-medium text-destructive"
          >
            {{ props.errors[day.weekday] }}
          </p>
        </div>
      </div>
    </div>
    <div>
      <Button
        v-if="empty"
        type="button"
        variant="fill"
        size="sm"
        @click="model = officeHours()"
      >
        {{ t('slas.editor.officeHours') }}
      </Button>
      <Button
        v-else
        type="button"
        variant="fill"
        size="sm"
        @click="model = slaWeekdays.map(() => [])"
      >
        {{ t('slas.editor.clearHours') }}
      </Button>
    </div>
  </div>
</template>
