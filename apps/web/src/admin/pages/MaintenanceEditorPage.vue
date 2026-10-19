<script setup lang="ts">
import { useMutation } from '@tanstack/vue-query'

import { ApiError } from '~/api'
import { maintenanceResource } from '~/api/resources/maintenance'
import SettingsPage from '~/components/account/SettingsPage.vue'
import SettingsSection from '~/components/account/SettingsSection.vue'
import CheckboxGroup from '~/components/CheckboxGroup.vue'
import CommandAlert from '~/components/CommandAlert.vue'
import FormField from '~/components/FormField.vue'
import WindowList from '~/components/maintenance/WindowList.vue'
import PageHeader from '~/components/PageHeader.vue'
import SelectField from '~/components/SelectField.vue'
import SwitchField from '~/components/SwitchField.vue'
import TextareaField from '~/components/TextareaField.vue'
import TimezoneField from '~/components/TimezoneField.vue'
import { Alert } from '~/components/ui/alert'
import { Button } from '~/components/ui/button'
import { Skeleton } from '~/components/ui/skeleton'
import { useErrorMessage } from '~/composables/useErrorMessage'
import { useMaintenanceDetail, useMaintenancePreview } from '~/composables/useMaintenance'
import { localTimezone, useMonitorTree } from '~/composables/useMonitors'
import { useCurrentNode, useNodeDetail } from '~/composables/useNodes'
import { usePageMeta } from '~/composables/usePageMeta'
import type { MaintenanceResult, MaintenanceStrategy } from '~/generated'
import { can } from '~/lib/access'
import {
  daysOfMonth,
  emptyMaintenanceForm,
  formFromMaintenance,
  previewSchedule,
  strategies,
  strategyLabels,
  toSchedule,
  usesDateRange,
  usesTimeOfDay,
  validateMaintenance,
  weekdays,
  type MaintenanceForm,
} from '~/lib/maintenance-form'
import { useI18n, type MessageKey } from '~/plugins/i18n'

/**
 * Plans maintenance on the node with one of Kuma's six strategies, or edits one. The next
 * windows show as the schedule is typed, read from the server so they match what will run,
 * DST changes included. Covered monitors report maintenance and send no alerts while a window
 * is active.
 */
const { t, locale } = useI18n()
const { fieldMessage, fieldProblem, message } = useErrorMessage()
const route = useRoute()
const router = useRouter()
const { routeId, tree } = useCurrentNode()
const node = useNodeDetail(routeId)
const manage = computed(() => can(node.data.value?.node, 'maintenance.manage'))

const maintenanceId = computed(() => (typeof route.params.maintenanceId === 'string' ? route.params.maintenanceId : null))
const editing = computed(() => maintenanceId.value !== null)
const existing = useMaintenanceDetail(maintenanceId)
const stored = computed(() => existing.data.value?.maintenance ?? null)
usePageMeta(() => (editing.value ? t('maintenance.editor.editTitle') : t('maintenance.editor.createTitle')))

const form = ref<MaintenanceForm>(emptyMaintenanceForm())
const basedOn = ref<number | null>(null)
const load = () => {
  const maintenance = stored.value
  if (!maintenance) return
  form.value = formFromMaintenance(maintenance)
  basedOn.value = maintenance.revision
}
watch(
  stored,
  () => {
    if (basedOn.value === null) load()
  },
  { immediate: true }
)

const monitors = useMonitorTree(routeId)
const monitorOptions = computed(() =>
  (monitors.data.value?.monitors ?? [])
    .map((monitor) => ({
      value: monitor.id,
      label: monitor.name,
      hint: monitor.node === routeId.value ? undefined : tree.index.value.byId.get(monitor.node)?.name,
    }))
    .sort((a, b) => a.label.localeCompare(b.label, locale.value))
)

const preview = useMaintenancePreview(() => previewSchedule(form.value))
const previewZone = computed(() => form.value.timezone.trim() || localTimezone())
const fieldLabels: Partial<Record<string, MessageKey>> = {
  cron: 'maintenance.editor.cron',
  timezone: 'maintenance.editor.timezone',
  start_date: 'maintenance.editor.start',
  end_date: 'maintenance.editor.end',
  start_time: 'maintenance.editor.startTime',
  end_time: 'maintenance.editor.endTime',
  duration: 'maintenance.editor.duration',
  interval_day: 'maintenance.editor.intervalDay',
}
/** What the server found wrong with the schedule, such as a cron expression it cannot read. */
const previewError = computed(() => {
  const error = preview.error.value
  if (!error) return null
  const field = error instanceof ApiError ? Object.keys(error.body.errors ?? {})[0] : undefined
  const problem = field ? fieldMessage(error, field) : null
  const label = field ? fieldLabels[field.replace(/^schedule\./, '')] : undefined
  return problem && label ? `${t(label)}: ${problem}` : (problem ?? message(error))
})

const submitted = ref(false)
const clientErrors = computed(() => (submitted.value ? validateMaintenance(form.value) : {}))
const save = useMutation({
  mutationFn: (current: MaintenanceForm): Promise<MaintenanceResult> => {
    const input = {
      title: current.title.trim(),
      description: current.description.trim() || undefined,
      schedule: toSchedule(current),
      monitors: current.monitors,
    }
    const maintenance = maintenanceId.value
    if (maintenance) return maintenanceResource.update({ ...input, maintenance, expected_revision: basedOn.value ?? undefined })
    return maintenanceResource.create({ ...input, node: routeId.value ?? '' })
  },
})
const errorFor = (key: string): string | null => {
  const local = clientErrors.value[key]
  if (local) return fieldProblem(local)
  return fieldMessage(save.error.value, key)
}

const submit = () => {
  submitted.value = true
  if (Object.keys(validateMaintenance(form.value)).length > 0) {
    void nextTick(() => document.querySelector<HTMLElement>('[aria-invalid="true"]')?.focus())
    return
  }
  save.mutate(form.value, {
    onSuccess: ({ maintenance }) => void router.push({ name: 'node-maintenance', params: { nodeId: maintenance.node } }),
  })
}

const reload = async () => {
  await existing.reload()
  basedOn.value = null
  load()
  save.reset()
}

const strategyHints: Record<MaintenanceStrategy, MessageKey> = {
  manual: 'maintenance.strategyHints.manual',
  single: 'maintenance.strategyHints.single',
  'recurring-interval': 'maintenance.strategyHints.recurring-interval',
  'recurring-weekday': 'maintenance.strategyHints.recurring-weekday',
  'recurring-day-of-month': 'maintenance.strategyHints.recurring-day-of-month',
  cron: 'maintenance.strategyHints.cron',
}
const strategyOptions = computed(() => strategies.map((value) => ({ value, label: t(strategyLabels[value]) })))

/** Weekday names in the reader's language; 7 January 2024 was a Sunday. */
const weekdayOptions = computed(() => {
  const format = new Intl.DateTimeFormat(locale.value, { weekday: 'long', timeZone: 'UTC' })
  return weekdays.map((day) => ({ value: String(day), label: format.format(Date.UTC(2024, 0, 7 + day)) }))
})
const pickedWeekdays = computed({
  get: () => form.value.weekdays.map(String),
  set: (next: string[]) => (form.value.weekdays = next.map(Number)),
})
const dayOptions = daysOfMonth.map((day) => ({ value: String(day), label: String(day) }))
const pickedDays = computed({
  get: () => form.value.daysOfMonth.map(String),
  set: (next: string[]) => (form.value.daysOfMonth = next.map(Number)),
})

const shownFields = [
  'title',
  'description',
  'monitors',
  'schedule.start_date',
  'schedule.end_date',
  'schedule.start_time',
  'schedule.end_time',
  'schedule.weekdays',
  'schedule.days_of_month',
  'schedule.interval_day',
  'schedule.cron',
  'schedule.duration',
  'schedule.timezone',
]
const ready = computed(() => node.data.value && (!editing.value || basedOn.value !== null))
</script>

<template>
  <div>
    <PageHeader
      :title="editing ? t('maintenance.editor.editTitle') : t('maintenance.editor.createTitle')"
      :description="stored?.title ?? node.data.value?.node.name"
    />

    <Alert
      v-if="(node.error.value && !node.data.value) || (existing.error.value && !stored)"
      color="destructive"
      class="max-w-6xl"
    >
      <p class="text-sm">{{ t('maintenance.unavailable') }}</p>
    </Alert>
    <div
      v-else-if="!ready"
      class="grid max-w-6xl gap-6"
    >
      <Skeleton class="h-32 w-full rounded-panel" />
      <Skeleton class="h-56 w-full rounded-panel" />
    </div>
    <Alert
      v-else-if="!manage"
      icon="lucide:info"
      class="max-w-6xl"
    >
      <p class="text-sm">{{ t('maintenance.readOnly') }}</p>
    </Alert>

    <form
      v-else
      class="grid max-w-6xl gap-x-10 gap-y-6 xl:grid-cols-[minmax(0,1fr)_20rem]"
      novalidate
      @submit.prevent="submit"
    >
      <div class="min-w-0">
        <SettingsPage>
          <SettingsSection
            id="maintenance-basics"
            :title="t('maintenance.editor.basics')"
          >
            <FormField
              id="maintenance-title"
              v-model="form.title"
              autocomplete="off"
              :label="t('maintenance.editor.title')"
              :error="errorFor('title')"
              required
            />
            <TextareaField
              id="maintenance-description"
              v-model="form.description"
              :rows="2"
              :label="t('maintenance.editor.description')"
              :error="errorFor('description')"
            />
          </SettingsSection>

          <SettingsSection
            id="maintenance-schedule"
            :title="t('maintenance.editor.schedule')"
          >
            <SelectField
              id="maintenance-strategy"
              v-model="form.strategy"
              :label="t('maintenance.editor.strategy')"
              :options="strategyOptions"
              :description="t(strategyHints[form.strategy])"
            />
            <div
              v-if="form.strategy === 'single'"
              class="grid gap-4 sm:grid-cols-2"
            >
              <FormField
                id="maintenance-start"
                v-model="form.startDate"
                type="datetime-local"
                :label="t('maintenance.editor.start')"
                :error="errorFor('schedule.start_date')"
                required
              />
              <FormField
                id="maintenance-end"
                v-model="form.endDate"
                type="datetime-local"
                :label="t('maintenance.editor.end')"
                :error="errorFor('schedule.end_date')"
                required
              />
            </div>
            <FormField
              v-if="form.strategy === 'recurring-interval'"
              id="maintenance-interval"
              v-model="form.intervalDay"
              inputmode="numeric"
              :label="t('maintenance.editor.intervalDay')"
              :description="t('maintenance.editor.intervalDayHint')"
              :error="errorFor('schedule.interval_day')"
              required
            />
            <CheckboxGroup
              v-if="form.strategy === 'recurring-weekday'"
              v-model="pickedWeekdays"
              :legend="t('maintenance.editor.weekdays')"
              :options="weekdayOptions"
              :error="errorFor('schedule.weekdays')"
            />
            <template v-if="form.strategy === 'recurring-day-of-month'">
              <fieldset class="grid gap-2">
                <legend class="mb-1 text-sm font-medium text-primary">{{ t('maintenance.editor.daysOfMonth') }}</legend>
                <div class="grid grid-cols-7 gap-1 rounded-lg border border-hairline p-2">
                  <label
                    v-for="day in dayOptions"
                    :key="day.value"
                    class="flex h-9 cursor-pointer items-center justify-center rounded-md text-sm tabular-nums transition-colors has-[:checked]:bg-primary has-[:checked]:text-primary-foreground has-[:focus-visible]:outline-2 has-[:focus-visible]:outline-ring hover:bg-fill"
                  >
                    <input
                      v-model="pickedDays"
                      type="checkbox"
                      class="sr-only"
                      :value="day.value"
                    >
                    {{ day.label }}
                  </label>
                </div>
                <p
                  v-if="errorFor('schedule.days_of_month')"
                  class="text-xs font-medium text-destructive"
                >
                  {{ errorFor('schedule.days_of_month') }}
                </p>
              </fieldset>
              <SwitchField
                id="maintenance-last-day"
                v-model="form.lastDayOfMonth"
                :label="t('maintenance.editor.lastDayOfMonth')"
              />
            </template>
            <template v-if="form.strategy === 'cron'">
              <FormField
                id="maintenance-cron"
                v-model="form.cron"
                autocomplete="off"
                :label="t('maintenance.editor.cron')"
                :description="t('maintenance.editor.cronHint')"
                :error="errorFor('schedule.cron')"
                required
              />
              <FormField
                id="maintenance-duration"
                v-model="form.durationMinutes"
                inputmode="numeric"
                :label="t('maintenance.editor.duration')"
                :error="errorFor('schedule.duration')"
                required
              />
            </template>
            <div
              v-if="usesTimeOfDay(form.strategy)"
              class="grid gap-4 sm:grid-cols-2"
            >
              <FormField
                id="maintenance-start-time"
                v-model="form.startTime"
                type="time"
                :label="t('maintenance.editor.startTime')"
                :error="errorFor('schedule.start_time')"
                required
              />
              <FormField
                id="maintenance-end-time"
                v-model="form.endTime"
                type="time"
                :label="t('maintenance.editor.endTime')"
                :description="t('maintenance.editor.endTimeHint')"
                :error="errorFor('schedule.end_time')"
                required
              />
            </div>
            <div
              v-if="usesDateRange(form.strategy)"
              class="grid gap-4 sm:grid-cols-2"
            >
              <FormField
                id="maintenance-range-start"
                v-model="form.startDate"
                type="datetime-local"
                :label="t('maintenance.editor.rangeStart')"
                :error="errorFor('schedule.start_date')"
              />
              <FormField
                id="maintenance-range-end"
                v-model="form.endDate"
                type="datetime-local"
                :label="t('maintenance.editor.rangeEnd')"
                :description="t('maintenance.editor.rangeHint')"
                :error="errorFor('schedule.end_date')"
              />
            </div>
            <TimezoneField
              id="maintenance-timezone"
              v-model="form.timezone"
              :label="t('maintenance.editor.timezone')"
              :description="t('maintenance.editor.timezoneHint')"
              :placeholder="t('maintenance.editor.timezonePlaceholder')"
              :error="errorFor('schedule.timezone')"
            />
          </SettingsSection>

          <SettingsSection
            id="maintenance-monitors"
            :title="t('maintenance.editor.monitors')"
            :description="t('maintenance.editor.monitorsHint')"
          >
            <Skeleton
              v-if="!monitors.data.value"
              class="h-32 w-full"
            />
            <p
              v-else-if="monitorOptions.length === 0"
              class="text-sm text-muted"
            >
              {{ t('maintenance.editor.noMonitors') }}
            </p>
            <CheckboxGroup
              v-else
              v-model="form.monitors"
              :legend="t('maintenance.editor.monitors')"
              :options="monitorOptions"
              :error="errorFor('monitors')"
            />
          </SettingsSection>
        </SettingsPage>

        <div class="mt-8 grid gap-4 border-t border-divider pt-6">
          <Alert
            v-if="submitted && Object.keys(clientErrors).length > 0"
            color="destructive"
          >
            <p class="text-sm">{{ t('maintenance.editor.invalid') }}</p>
          </Alert>
          <CommandAlert
            v-else-if="save.error.value"
            :error="save.error.value"
            :shown="shownFields"
            @reload="reload"
          />
          <div class="flex flex-wrap gap-2">
            <Button
              type="submit"
              variant="accent"
              :loading="save.isPending.value"
            >
              {{ editing ? t('maintenance.editor.saveEdit') : t('maintenance.editor.saveCreate') }}
            </Button>
            <Button
              as-child
              variant="fill"
            >
              <RouterLink :to="{ name: 'node-maintenance', params: { nodeId: routeId } }">{{ t('actions.cancel') }}</RouterLink>
            </Button>
          </div>
        </div>
      </div>

      <aside
        class="grid content-start gap-3 rounded-panel border border-hairline bg-card p-4.5 xl:sticky xl:top-24"
        aria-labelledby="maintenance-preview-title"
        aria-live="polite"
      >
        <h2
          id="maintenance-preview-title"
          class="text-base font-semibold text-primary"
        >
          {{ t('maintenance.preview.title') }}
        </h2>
        <p class="text-xs text-muted">{{ t('maintenance.preview.zone', { zone: previewZone }) }}</p>
        <p
          v-if="form.strategy === 'manual'"
          class="text-sm text-muted"
        >
          {{ t('maintenance.preview.manual') }}
        </p>
        <p
          v-else-if="!previewSchedule(form)"
          class="text-sm text-muted"
        >
          {{ t('maintenance.preview.incomplete') }}
        </p>
        <p
          v-else-if="previewError"
          class="text-sm text-destructive"
        >
          {{ previewError }}
        </p>
        <Skeleton
          v-else-if="!preview.data.value"
          class="h-24 w-full"
        />
        <p
          v-else-if="preview.data.value.windows.length === 0"
          class="text-sm text-muted"
        >
          {{ t('maintenance.preview.none') }}
        </p>
        <WindowList
          v-else
          data-testid="maintenance-preview"
          :windows="preview.data.value.windows"
          :timezone="previewZone"
        />
      </aside>
    </form>
  </div>
</template>
