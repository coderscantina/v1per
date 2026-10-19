<script setup lang="ts">
import Icon from '~/components/Icon.vue'
import { Checkbox } from '~/components/ui/checkbox'
import { Input } from '~/components/ui/input'
import type { TargetOption } from '~/composables/useIncidents'
import { selectTargets, type ImpactSelection, type TargetKey } from '~/lib/incidents'
import { useI18n } from '~/plugins/i18n'

import ImpactSelect from './ImpactSelect.vue'

/**
 * Picks the monitors and static components an incident names, each with its impact unless
 * `impact` is off. Picked targets stay on top; a filter narrows long lists by name.
 */
const props = defineProps<{
  legend: string
  options: readonly TargetOption[]
  impact?: boolean
  error?: string | null
  /** Names the node a target sits on, when it is below the incident's own. */
  nodeName?: (node: string) => string | undefined
}>()
const model = defineModel<ImpactSelection>({ required: true })

const { t } = useI18n()
const id = useId()
const search = ref('')

const picked = computed(() => Object.keys(model.value) as TargetKey[])
const isPicked = (key: TargetKey) => key in model.value
const toggle = (key: TargetKey, on: boolean | 'indeterminate') => {
  model.value = selectTargets(model.value, on === true ? [...picked.value, key] : picked.value.filter((entry) => entry !== key))
}
const setImpact = (key: TargetKey, impact: ImpactSelection[TargetKey]) => {
  if (impact) model.value = { ...model.value, [key]: impact }
}

const fold = (text: string) => text.normalize('NFD').replace(/\p{Diacritic}/gu, '').toLowerCase()
const rows = computed(() => {
  const needle = fold(search.value.trim())
  const matching = props.options.filter((option) => isPicked(option.key) || !needle || fold(option.name).includes(needle))
  return [...matching.filter((option) => isPicked(option.key)), ...matching.filter((option) => !isPicked(option.key))]
})
</script>

<template>
  <fieldset
    class="grid gap-2"
    :aria-describedby="error ? `${id}-error` : undefined"
  >
    <legend class="mb-1 text-sm font-medium text-primary">{{ legend }}</legend>
    <div
      v-if="options.length > 8"
      class="relative"
    >
      <Icon
        name="lucide:search"
        size="14"
        class="pointer-events-none absolute top-1/2 left-3 -translate-y-1/2 text-muted"
        aria-hidden="true"
      />
      <Input
        v-model="search"
        type="search"
        class="pl-8"
        :placeholder="t('incidents.targets.search')"
        :aria-label="t('incidents.targets.search')"
      />
    </div>
    <p
      v-if="options.length === 0"
      class="rounded-lg border border-hairline p-3 text-sm text-muted"
    >
      {{ t('incidents.targets.none') }}
    </p>
    <ul
      v-else
      class="grid max-h-96 divide-y divide-divider overflow-y-auto rounded-lg border border-hairline"
    >
      <li
        v-for="option in rows"
        :key="option.key"
        class="flex flex-wrap items-center gap-x-3 gap-y-2 px-3 py-2"
        :data-target="option.name"
      >
        <label class="flex min-w-0 flex-1 items-center gap-2 text-sm text-primary">
          <Checkbox
            :model-value="isPicked(option.key)"
            @update:model-value="(on) => toggle(option.key, on)"
          />
          <Icon
            :name="option.kind === 'monitor' ? 'lucide:activity' : 'lucide:box'"
            size="14"
            class="shrink-0 text-muted"
            aria-hidden="true"
          />
          <span class="grid min-w-0">
            <span class="truncate">{{ option.name }}</span>
            <span class="text-xs text-muted">
              {{ option.kind === 'monitor' ? t('incidents.targets.monitor') : t('incidents.targets.component') }}<template
                v-if="nodeName?.(option.node)"
              > · {{ nodeName(option.node) }}</template>
            </span>
          </span>
        </label>
        <ImpactSelect
          v-if="impact && isPicked(option.key)"
          :model-value="model[option.key]"
          :label="t('incidents.targets.impactOf', { name: option.name })"
          @update:model-value="(value) => setImpact(option.key, value)"
        />
      </li>
    </ul>
    <p
      v-if="error"
      :id="`${id}-error`"
      class="text-xs font-medium text-destructive"
    >
      {{ error }}
    </p>
  </fieldset>
</template>
