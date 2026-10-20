<script setup lang="ts">
import { RadioGroupItem, RadioGroupRoot } from 'reka-ui'

import SettingsPage from '~/components/account/SettingsPage.vue'
import SettingsSection from '~/components/account/SettingsSection.vue'
import ChoiceGroup from '~/components/ChoiceGroup.vue'
import SelectField from '~/components/SelectField.vue'
import { accentColors } from '~/composables/useAccentColor'
import { colorModes, useColorMode } from '~/composables/useColorMode'
import { useDateFormat } from '~/composables/useDateFormat'
import { localTimezone } from '~/composables/useMonitors'
import { usePageMeta } from '~/composables/usePageMeta'
import { useAccentPreference, useFormats, usePreferences, useStatusCardPreference } from '~/composables/usePreferences'
import type { DateFormat, DisplayZone, NumberFormat, Preferences, StatusCardStyle, TimeFormat, WeekStart } from '~/generated'
import { formatDate, formatNumber, formatTime, numberLocale, weekStart } from '~/lib/formats'
import { formatLatency, formatUptime } from '~/lib/monitor-status'
import { isSupportedLocale, useI18n, type MessageKey } from '~/plugins/i18n'

/**
 * How the app looks and writes things for the signed-in person: theme and accent, the monitor
 * page's status card, language, and number, date and time formats. The accent, card and formats
 * are saved to their preferences and follow them to every browser; theme and language stay in
 * this browser. Every change applies at once.
 */
const { t, locale, locales, setLocale } = useI18n()
usePageMeta(() => t('account.appearance.title'))

const { mode, setMode } = useColorMode()
const { accent, pick } = useAccentPreference()
const cardStyle = useStatusCardPreference()
const { query, save } = usePreferences()
const { formats } = useFormats()
const { relative } = useDateFormat()

const pickAccent = (value: unknown) => {
  const next = accentColors.find((entry) => entry === value)
  if (next) pick(next)
}
const modeOptions = computed(() => colorModes.map((value) => ({ value, label: t(`colorMode.${value}`) })))
const colorMode = computed({ get: () => mode.value, set: setMode })

const cards: Array<{ value: StatusCardStyle; label: MessageKey; description: MessageKey }> = [
  { value: 'banner', label: 'monitors.cardStyles.banner', description: 'account.appearance.cards.banner' },
  { value: 'hero', label: 'monitors.cardStyles.hero', description: 'account.appearance.cards.hero' },
  { value: 'compact', label: 'monitors.cardStyles.compact', description: 'account.appearance.cards.compact' },
  { value: 'gauge', label: 'monitors.cardStyles.gauge', description: 'account.appearance.cards.gauge' },
]
const pickCard = (value: unknown) => {
  const card = cards.find((entry) => entry.value === value)
  if (card) cardStyle.value = card.value
}

const language = computed({
  get: () => locale.value,
  set: (value: string) => {
    if (isSupportedLocale(value)) setLocale(value)
  },
})
const languageOptions = computed(() => locales.map((entry) => ({ value: entry.code, label: entry.name })))

/** Select items cannot carry an empty value, so following the language is this one. */
const AUTO = 'auto'
const SAMPLE_NUMBER = 1234.5
const sampleDay = Date.UTC(2026, 9, 20, 14, 30)
const plain = computed(() => ({ language: locale.value }))
const byLanguage = (example: string) => t('account.appearance.byLanguage', { example })

/** A select bound to one preference, absent while it follows the language. */
function preference<Key extends 'number_format' | 'date_format' | 'time_format' | 'time_zone' | 'week_start', Value extends NonNullable<Preferences[Key]>>(
  key: Key,
  values: readonly Value[]
) {
  return computed({
    get: (): string => query.data.value?.[key] ?? AUTO,
    set: (value: string) => save({ [key]: values.find((entry) => entry === value) }),
  })
}

const numberFormats = ['comma', 'period', 'space', 'space_period', 'apostrophe'] as const satisfies readonly NumberFormat[]
const numberFormat = preference('number_format', numberFormats)
const numberOptions = computed(() => [
  { value: AUTO, label: byLanguage(new Intl.NumberFormat(locale.value).format(SAMPLE_NUMBER)) },
  ...numberFormats.map((value) => ({
    value,
    label: formatNumber(numberLocale({ language: locale.value, number: value }), SAMPLE_NUMBER),
  })),
])

const dateFormats = ['day_month_year', 'month_day_year', 'year_month_day'] as const satisfies readonly DateFormat[]
const dateFormat = preference('date_format', dateFormats)
const dateOptions = computed(() => [
  { value: AUTO, label: byLanguage(formatDate(plain.value, sampleDay, 'UTC')) },
  ...dateFormats.map((value) => ({ value, label: formatDate({ language: locale.value, date: value }, sampleDay, 'UTC') })),
])

const timeFormats = ['h24', 'h12'] as const satisfies readonly TimeFormat[]
const timeFormat = preference('time_format', timeFormats)
const timeOptions = computed(() => [
  { value: AUTO, label: byLanguage(formatTime(plain.value, sampleDay, 'UTC')) },
  ...timeFormats.map((value) => ({ value, label: formatTime({ language: locale.value, time: value }, sampleDay, 'UTC') })),
])

const weekStarts = ['monday', 'saturday', 'sunday'] as const satisfies readonly WeekStart[]
const week = preference('week_start', weekStarts)
/** 7 January 2024 was a Sunday. */
const weekdayName = (day: number) =>
  new Intl.DateTimeFormat(locale.value, { weekday: 'long', timeZone: 'UTC' }).format(Date.UTC(2024, 0, 7 + day))
const weekOptions = computed(() => [
  { value: AUTO, label: byLanguage(weekdayName(weekStart(plain.value))) },
  ...weekStarts.map((value) => ({ value, label: weekdayName(weekStart({ language: locale.value, week: value })) })),
])

const zones = ['local', 'utc'] as const satisfies readonly DisplayZone[]
const zone = computed({
  get: (): string => query.data.value?.time_zone ?? 'local',
  set: (value: string) => save({ time_zone: zones.find((entry) => entry === value) }),
})
const zoneOptions = computed(() => [
  { value: 'local', label: t('account.appearance.zoneLocal', { zone: localTimezone() }) },
  { value: 'utc', label: t('account.appearance.zoneUtc') },
])

/** The choices applied together, as the app now writes them. */
const preview = computed(() => {
  return {
    when: `${formatDate(formats.value, Date.now())}, ${formatTime(formats.value, Date.now())}`,
    latency: formatLatency(SAMPLE_NUMBER, numberLocale(formats.value)),
    uptime: formatUptime(0.9998, numberLocale(formats.value)),
    ago: relative(Date.now() - 90_000, Date.now()),
  }
})
</script>

<template>
  <SettingsPage>
    <SettingsSection
      id="theme"
      :title="t('account.appearance.theme')"
      :description="t('account.appearance.themeDescription')"
    >
      <div class="grid gap-1.5">
        <span class="text-sm font-medium text-primary">{{ t('account.appearance.mode') }}</span>
        <ChoiceGroup
          v-model="colorMode"
          :label="t('account.appearance.mode')"
          :options="modeOptions"
          class="justify-self-start"
        />
      </div>
      <div class="grid gap-1.5">
        <span class="text-sm font-medium text-primary">{{ t('accent.label') }}</span>
        <RadioGroupRoot
          :model-value="accent"
          :aria-label="t('accent.label')"
          class="flex flex-wrap gap-2"
          @update:model-value="pickAccent"
        >
          <RadioGroupItem
            v-for="option in accentColors"
            :key="option"
            :value="option"
            :data-accent="option"
            :aria-label="t(`accent.${option}`)"
            :title="t(`accent.${option}`)"
            class="size-8 cursor-pointer rounded-full bg-accent ring-offset-2 ring-offset-background transition-shadow focus-visible:outline-2 focus-visible:outline-offset-4 focus-visible:outline-ring data-[state=checked]:ring-2 data-[state=checked]:ring-primary"
          />
        </RadioGroupRoot>
      </div>
    </SettingsSection>

    <SettingsSection
      id="status-card"
      :title="t('account.appearance.statusCard')"
      :description="t('account.appearance.statusCardDescription')"
    >
      <RadioGroupRoot
        :model-value="cardStyle"
        :aria-label="t('account.appearance.statusCard')"
        class="grid gap-3 @lg:grid-cols-2"
        @update:model-value="pickCard"
      >
        <RadioGroupItem
          v-for="card in cards"
          :key="card.value"
          :value="card.value"
          class="grid cursor-pointer content-start gap-3 rounded-xl border border-hairline bg-surface p-3.5 text-left transition-colors hover:border-border-strong focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring data-[state=checked]:border-accent data-[state=checked]:bg-accent/8"
        >
          <span
            class="grid h-16 overflow-hidden rounded-lg border border-hairline bg-card p-2.5"
            aria-hidden="true"
          >
            <span
              v-if="card.value === 'banner'"
              class="grid content-between rounded-md border border-status-up/30 bg-status-up/10 p-2"
            >
              <span class="h-2 w-12 rounded-full bg-status-up" />
              <span class="flex gap-2"><span
                v-for="n in 4"
                :key="n"
                class="h-1.5 flex-1 rounded-full bg-fill-strong"
              /></span>
            </span>
            <span
              v-else-if="card.value === 'hero'"
              class="flex items-center justify-between gap-3"
            >
              <span class="h-5 w-20 rounded-md bg-status-up" />
              <span class="grid grid-cols-2 gap-1.5"><span
                v-for="n in 4"
                :key="n"
                class="h-2.5 w-8 rounded-sm bg-fill-strong"
              /></span>
            </span>
            <span
              v-else-if="card.value === 'compact'"
              class="flex items-center gap-2 self-center rounded-md border border-status-up/30 bg-status-up/10 px-2 py-1.5"
            >
              <span class="size-2 rounded-full bg-status-up" />
              <span class="h-1.5 flex-1 rounded-full bg-fill-strong" />
              <span class="h-1.5 w-8 rounded-full bg-fill-strong" />
            </span>
            <span
              v-else
              class="flex items-center gap-3"
            >
              <span class="grid size-11 place-items-center rounded-full border-4 border-status-up">
                <span class="grid size-6 place-items-center rounded-full border-[3px] border-status-up/70" />
              </span>
              <span class="grid flex-1 gap-1.5"><span
                v-for="n in 2"
                :key="n"
                class="h-1.5 rounded-full bg-fill-strong"
              /></span>
            </span>
          </span>
          <span class="grid gap-0.5">
            <span class="text-sm font-semibold text-primary">{{ t(card.label) }}</span>
            <span class="text-sm text-muted">{{ t(card.description) }}</span>
          </span>
        </RadioGroupItem>
      </RadioGroupRoot>
    </SettingsSection>

    <SettingsSection
      id="formats"
      :title="t('account.appearance.formats')"
      :description="t('account.appearance.formatsDescription')"
    >
      <div class="grid gap-4 @lg:grid-cols-2">
        <SelectField
          id="appearance-language"
          v-model="language"
          :label="t('shell.language')"
          :options="languageOptions"
        />
        <SelectField
          id="appearance-zone"
          v-model="zone"
          :label="t('account.appearance.zone')"
          :options="zoneOptions"
        />
        <SelectField
          id="appearance-number"
          v-model="numberFormat"
          :label="t('account.appearance.number')"
          :options="numberOptions"
        />
        <SelectField
          id="appearance-date"
          v-model="dateFormat"
          :label="t('account.appearance.date')"
          :options="dateOptions"
        />
        <SelectField
          id="appearance-time"
          v-model="timeFormat"
          :label="t('account.appearance.time')"
          :options="timeOptions"
        />
        <SelectField
          id="appearance-week"
          v-model="week"
          :label="t('account.appearance.week')"
          :options="weekOptions"
        />
      </div>
      <dl
        class="grid grid-cols-2 gap-x-6 gap-y-3 rounded-xl border border-hairline bg-surface p-4 @lg:grid-cols-4"
        :aria-label="t('account.appearance.preview')"
      >
        <div
          v-for="(value, key) in preview"
          :key="key"
          class="grid min-w-0 gap-0.5"
        >
          <dt class="text-[0.8125rem] text-muted">{{ t(`account.appearance.previewLabels.${key}`) }}</dt>
          <dd class="text-base font-semibold break-words text-primary tabular-nums">{{ value }}</dd>
        </div>
      </dl>
    </SettingsSection>
  </SettingsPage>
</template>
