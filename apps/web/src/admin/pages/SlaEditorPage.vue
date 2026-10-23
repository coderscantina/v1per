<script setup lang="ts">
import { useMutation } from '@tanstack/vue-query'

import { slasResource } from '~/api/resources/slas'
import SettingsPage from '~/components/account/SettingsPage.vue'
import SettingsSection from '~/components/account/SettingsSection.vue'
import CheckboxGroup from '~/components/CheckboxGroup.vue'
import ChoiceGroup from '~/components/ChoiceGroup.vue'
import CommandAlert from '~/components/CommandAlert.vue'
import FormField from '~/components/FormField.vue'
import Icon from '~/components/Icon.vue'
import PageHeader from '~/components/PageHeader.vue'
import SlaHoursEditor from '~/components/slas/SlaHoursEditor.vue'
import TimezoneField from '~/components/TimezoneField.vue'
import { Alert } from '~/components/ui/alert'
import { Button } from '~/components/ui/button'
import { Input } from '~/components/ui/input'
import { Skeleton } from '~/components/ui/skeleton'
import { useAvailableChannels } from '~/composables/useChannels'
import { useErrorMessage } from '~/composables/useErrorMessage'
import { localTimezone, useMonitorTree } from '~/composables/useMonitors'
import { useCurrentNode, useNodeDetail } from '~/composables/useNodes'
import { usePageMeta } from '~/composables/usePageMeta'
import { useFormats } from '~/composables/usePreferences'
import { useSlaDetail } from '~/composables/useSlas'
import type { SlaResult } from '~/generated'
import { can } from '~/lib/access'
import { providerName } from '~/lib/channels'
import { formatDate } from '~/lib/formats'
import { impactLabels } from '~/lib/incidents'
import {
  combineHints,
  combineLabels,
  combineModes,
  emptySlaForm,
  formFromSla,
  periodKinds,
  periodLabels,
  SLA_NAME_MAX_CHARS,
  slaImpacts,
  slaWeekdays,
  toSlaInput,
  validateSla,
  type SlaForm,
  type SlaImpact,
} from '~/lib/sla-form'
import { useI18n, type MessageKey } from '~/plugins/i18n'

/**
 * Adds an SLA to the node or edits one: its period and timezone, the covered monitors and how
 * their availability combines, the four optional targets, business hours with holidays for the
 * incident clocks, and the channels that hear about the budget and breaches. Only staff with
 * Admin above the node (`slas.manage`) get the form; everyone else reads the list.
 */
const { t, locale } = useI18n()
const { formats } = useFormats()
const { fieldMessage, fieldProblem } = useErrorMessage()
const route = useRoute()
const router = useRouter()
const { routeId, tree } = useCurrentNode()
const node = useNodeDetail(routeId)
const manage = computed(() => can(node.data.value?.node, 'slas.manage'))

const slaId = computed(() => (typeof route.params.slaId === 'string' ? route.params.slaId : null))
const editing = computed(() => slaId.value !== null)
const existing = useSlaDetail(slaId)
const stored = computed(() => existing.data.value?.sla ?? null)
usePageMeta(() => (editing.value ? t('slas.editor.editTitle') : t('slas.editor.createTitle')))

const form = ref<SlaForm>(emptySlaForm(localTimezone()))
const basedOn = ref<number | null>(null)
const load = () => {
  const sla = stored.value
  if (!sla) return
  form.value = formFromSla(sla)
  basedOn.value = sla.revision
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

const channels = useAvailableChannels(routeId)
const channelOptions = computed(() =>
  (channels.data.value?.channels ?? []).map((choice) => ({
    value: choice.id,
    label: choice.name,
    hint: [providerName(choice.provider, t), choice.active ? null : t('channels.inactive')]
      .filter((part) => part !== null)
      .join(' · '),
  }))
)

const periodOptions = computed(() => periodKinds.map((value) => ({ value, label: t(periodLabels[value]) })))
const combineOptions = computed(() => combineModes.map((value) => ({ value, label: t(combineLabels[value]) })))

const submitted = ref(false)
const clientErrors = computed(() => (submitted.value ? validateSla(form.value) : {}))
const save = useMutation({
  mutationFn: (current: SlaForm): Promise<SlaResult> => {
    const input = toSlaInput(current)
    const sla = slaId.value
    if (sla) return slasResource.update({ ...input, sla, expected_revision: basedOn.value ?? undefined })
    return slasResource.create({ ...input, node: routeId.value ?? '' })
  },
})

/** Problems the generic wording would leave unclear. */
const specific: Partial<Record<string, MessageKey>> = { availability: 'slas.editor.availabilityInvalid' }
const errorFor = (key: string): string | null => {
  const local = clientErrors.value[key]
  if (local) {
    const message = local.code === 'invalid_value' ? specific[key] : undefined
    return message ? t(message) : fieldProblem(local)
  }
  return fieldMessage(save.error.value, key)
}
const dayErrors = computed(() =>
  slaWeekdays.map((weekday) => (clientErrors.value[`hours.${weekday}`] ? t('slas.editor.hoursInvalid') : null))
)

const submit = () => {
  submitted.value = true
  if (Object.keys(validateSla(form.value)).length > 0) {
    void nextTick(() => document.querySelector<HTMLElement>('[aria-invalid="true"]')?.focus())
    return
  }
  save.mutate(form.value, {
    onSuccess: ({ sla }) => void router.push({ name: 'node-slas', params: { nodeId: sla.node } }),
  })
}

const reload = async () => {
  await existing.reload()
  basedOn.value = null
  load()
  save.reset()
}

const holiday = ref('')
const addHoliday = () => {
  const day = holiday.value
  if (!/^\d{4}-\d{2}-\d{2}$/.test(day)) return
  if (!form.value.holidays.includes(day)) form.value.holidays = [...form.value.holidays, day].sort()
  holiday.value = ''
}
const removeHoliday = (day: string) => (form.value.holidays = form.value.holidays.filter((entry) => entry !== day))
/** A holiday is a calendar day, so it is written without shifting it into another zone. */
const holidayLabel = (day: string) => formatDate(formats.value, Date.parse(`${day}T00:00:00Z`), 'UTC')

const clocks = [
  { key: 'response', field: 'response_minutes', label: 'slas.editor.responseFor' },
  { key: 'resolution', field: 'resolution_minutes', label: 'slas.editor.resolutionFor' },
] as const satisfies { key: keyof SlaForm['times'][SlaImpact]; field: string; label: MessageKey }[]

const shownFields = [
  'name',
  'timezone',
  'start_date',
  'availability',
  'latency_ms',
  ...slaImpacts.flatMap((impact) => [`times.${impact}.response_minutes`, `times.${impact}.resolution_minutes`]),
  'hours',
  'holidays',
  'monitors',
  'channels',
]
const ready = computed(() => node.data.value && (!editing.value || basedOn.value !== null))
</script>

<template>
  <div>
    <PageHeader
      :title="editing ? t('slas.editor.editTitle') : t('slas.editor.createTitle')"
      :description="stored?.name ?? node.data.value?.node.name"
    />

    <Alert
      v-if="(node.error.value && !node.data.value) || (existing.error.value && !stored)"
      color="destructive"
      class="max-w-4xl"
    >
      <p class="text-sm">{{ t('slas.unavailable') }}</p>
    </Alert>
    <div
      v-else-if="!ready"
      class="grid max-w-4xl gap-6"
    >
      <Skeleton class="h-32 w-full rounded-panel" />
      <Skeleton class="h-56 w-full rounded-panel" />
    </div>
    <Alert
      v-else-if="!manage"
      icon="lucide:info"
      class="max-w-4xl"
    >
      <p class="text-sm">{{ t('slas.readOnly') }}</p>
    </Alert>

    <form
      v-else
      class="max-w-4xl"
      novalidate
      @submit.prevent="submit"
    >
      <SettingsPage>
        <SettingsSection
          id="sla-basics"
          :title="t('slas.editor.basics')"
          :description="t('slas.editor.basicsHint')"
        >
          <FormField
            id="sla-name"
            v-model="form.name"
            autocomplete="off"
            :label="t('slas.editor.name')"
            :maxlength="SLA_NAME_MAX_CHARS"
            :error="errorFor('name')"
            required
          />
          <div class="grid gap-2">
            <p class="text-sm font-medium text-primary">{{ t('slas.editor.period') }}</p>
            <ChoiceGroup
              v-model="form.period"
              :label="t('slas.editor.period')"
              :options="periodOptions"
              class="w-fit"
            />
          </div>
          <div class="grid gap-4 sm:grid-cols-2">
            <FormField
              id="sla-start"
              v-model="form.startDate"
              type="date"
              :label="t('slas.editor.startDate')"
              :error="errorFor('start_date')"
              required
            />
            <TimezoneField
              id="sla-timezone"
              v-model="form.timezone"
              :label="t('slas.editor.timezone')"
              :error="errorFor('timezone')"
            />
          </div>
        </SettingsSection>

        <SettingsSection
          id="sla-monitors"
          :title="t('slas.editor.monitors')"
          :description="t('slas.editor.monitorsHint')"
        >
          <div class="grid gap-2">
            <p class="text-sm font-medium text-primary">{{ t('slas.editor.combine') }}</p>
            <ChoiceGroup
              v-model="form.combine"
              :label="t('slas.editor.combine')"
              :options="combineOptions"
              class="w-fit max-w-full"
            />
            <p class="text-sm text-muted">{{ t(combineHints[form.combine]) }}</p>
          </div>
          <Skeleton
            v-if="!monitors.data.value"
            class="h-32 w-full"
          />
          <p
            v-else-if="monitorOptions.length === 0"
            class="text-sm text-muted"
          >
            {{ t('slas.editor.noMonitors') }}
          </p>
          <CheckboxGroup
            v-else
            v-model="form.monitors"
            :legend="t('slas.editor.monitors')"
            :options="monitorOptions"
            :error="errorFor('monitors')"
          />
        </SettingsSection>

        <SettingsSection
          id="sla-targets"
          :title="t('slas.editor.targets')"
          :description="t('slas.editor.targetsHint')"
        >
          <div class="grid gap-4 sm:grid-cols-2">
            <FormField
              id="sla-availability"
              v-model="form.availability"
              inputmode="decimal"
              :label="t('slas.editor.availability')"
              suffix="%"
              :error="errorFor('availability')"
            />
            <FormField
              id="sla-latency"
              v-model="form.latencyMs"
              inputmode="numeric"
              :label="t('slas.editor.latency')"
              suffix="ms"
              :error="errorFor('latency_ms')"
            />
          </div>
          <div class="grid gap-3">
            <div>
              <p class="text-sm font-medium text-primary">{{ t('slas.editor.times') }}</p>
              <p class="text-sm text-muted">{{ t('slas.editor.timesHint') }}</p>
            </div>
            <div
              class="grid grid-cols-2 items-center gap-x-4 gap-y-2 sm:grid-cols-[minmax(0,10rem)_minmax(0,1fr)_minmax(0,1fr)]"
              role="group"
              :aria-label="t('slas.editor.times')"
            >
              <span
                class="max-sm:hidden"
                aria-hidden="true"
              />
              <span class="text-sm font-medium text-primary">{{ t('slas.editor.response') }}</span>
              <span class="text-sm font-medium text-primary">{{ t('slas.editor.resolution') }}</span>
              <template
                v-for="impact in slaImpacts"
                :key="impact"
              >
                <span class="text-sm text-primary max-sm:col-span-2 max-sm:mt-1">{{ t(impactLabels[impact]) }}</span>
                <FormField
                  v-for="clock in clocks"
                  :id="`sla-${impact}-${clock.key}`"
                  :key="clock.key"
                  v-model="form.times[impact][clock.key]"
                  inputmode="numeric"
                  :label="t(clock.label, { impact: t(impactLabels[impact]) })"
                  :suffix="t('units.minutes')"
                  :error="errorFor(`times.${impact}.${clock.field}`)"
                  class="[&>label]:sr-only"
                />
              </template>
            </div>
          </div>
        </SettingsSection>

        <SettingsSection
          id="sla-hours"
          :title="t('slas.editor.hours')"
          :description="t('slas.editor.hoursHint')"
        >
          <SlaHoursEditor
            v-model="form.hours"
            :errors="dayErrors"
          />
          <p
            v-if="errorFor('hours')"
            class="text-sm font-medium text-destructive"
          >
            {{ errorFor('hours') }}
          </p>
          <div class="grid gap-2">
            <div>
              <p class="text-sm font-medium text-primary">{{ t('slas.editor.holidays') }}</p>
              <p class="text-sm text-muted">{{ t('slas.editor.holidaysHint') }}</p>
            </div>
            <div class="flex items-center gap-2">
              <Input
                v-model="holiday"
                type="date"
                class="w-44"
                :aria-label="t('slas.editor.holidayDate')"
                @keydown.enter.prevent="addHoliday"
              />
              <Button
                type="button"
                variant="fill"
                :disabled="holiday === ''"
                @click="addHoliday"
              >
                <Icon name="lucide:plus" />
                {{ t('slas.editor.addHoliday') }}
              </Button>
            </div>
            <p
              v-if="form.holidays.length === 0"
              class="text-sm text-muted"
            >
              {{ t('slas.editor.noHolidays') }}
            </p>
            <ul
              v-else
              class="flex flex-wrap gap-1.5"
            >
              <li
                v-for="day in form.holidays"
                :key="day"
                class="flex h-7 items-center gap-1 rounded-md bg-fill pr-0.5 pl-2.5 text-sm text-primary tabular-nums"
              >
                {{ holidayLabel(day) }}
                <Button
                  type="button"
                  variant="ghost"
                  size="toolbar"
                  :aria-label="t('slas.editor.removeHoliday', { date: holidayLabel(day) })"
                  @click="removeHoliday(day)"
                >
                  <Icon name="lucide:x" />
                </Button>
              </li>
            </ul>
            <p
              v-if="errorFor('holidays')"
              class="text-sm font-medium text-destructive"
            >
              {{ errorFor('holidays') }}
            </p>
          </div>
        </SettingsSection>

        <SettingsSection
          id="sla-channels"
          :title="t('slas.editor.channels')"
          :description="t('slas.editor.channelsHint')"
        >
          <Skeleton
            v-if="!channels.data.value"
            class="h-20 w-full"
          />
          <p
            v-else-if="channelOptions.length === 0"
            class="text-sm text-muted"
          >
            {{ t('slas.editor.noChannels') }}
          </p>
          <CheckboxGroup
            v-else
            v-model="form.channels"
            :legend="t('slas.editor.channels')"
            :options="channelOptions"
            :error="errorFor('channels')"
          />
        </SettingsSection>
      </SettingsPage>

      <div class="mt-8 grid gap-4 border-t border-divider pt-6">
        <Alert
          v-if="submitted && Object.keys(clientErrors).length > 0"
          color="destructive"
        >
          <p class="text-sm">{{ t('slas.editor.invalid') }}</p>
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
            {{ editing ? t('slas.editor.saveEdit') : t('slas.editor.saveCreate') }}
          </Button>
          <Button
            as-child
            variant="fill"
          >
            <RouterLink :to="{ name: 'node-slas', params: { nodeId: routeId } }">{{ t('actions.cancel') }}</RouterLink>
          </Button>
        </div>
      </div>
    </form>
  </div>
</template>
