<script setup lang="ts" generic="T extends string">
// Adapted from holocron (84fc54ec): apps/web/src/components/CheckboxGroup.vue. Options can be
// unavailable, with the reason under their label; "select all" picks the available ones.
import { Checkbox } from '~/components/ui/checkbox'
import { useI18n } from '~/plugins/i18n'

const props = defineProps<{
  legend: string
  options: { value: T; label: string; hint?: string; disabled?: boolean }[]
  error?: string | null
}>()
const model = defineModel<T[]>({ required: true })

const { t } = useI18n()
const id = useId()

const available = computed(() => props.options.filter((option) => !option.disabled).map((option) => option.value))
const all = computed(() => available.value.length > 0 && available.value.every((value) => model.value.includes(value)))

const toggle = (value: T, on: boolean | 'indeterminate') => {
  model.value = on === true ? [...model.value, value] : model.value.filter((entry) => entry !== value)
}
const toggleAll = (on: boolean | 'indeterminate') => {
  model.value = on === true ? [...available.value] : []
}
</script>

<template>
  <fieldset
    class="grid gap-2"
    :aria-describedby="error ? `${id}-error` : undefined"
  >
    <legend class="mb-1 text-sm font-medium text-primary">{{ legend }}</legend>
    <label class="flex w-fit items-center gap-2 text-xs text-muted">
      <Checkbox
        :model-value="all"
        :disabled="available.length === 0"
        @update:model-value="toggleAll"
      />
      {{ t('actions.selectAll') }}
    </label>
    <div class="grid gap-2 rounded-lg border border-hairline p-3 sm:grid-cols-2">
      <label
        v-for="option in options"
        :key="option.value"
        class="flex min-w-0 items-start gap-2 text-sm"
        :class="option.disabled ? 'text-muted' : 'text-primary'"
      >
        <Checkbox
          class="mt-0.5"
          :model-value="model.includes(option.value)"
          :disabled="option.disabled"
          @update:model-value="(on) => toggle(option.value, on)"
        />
        <span class="grid min-w-0">
          <span class="truncate">{{ option.label }}</span>
          <span
            v-if="option.hint"
            class="text-xs text-muted"
          >
            {{ option.hint }}
          </span>
        </span>
      </label>
    </div>
    <p
      v-if="error"
      :id="`${id}-error`"
      class="text-xs font-medium text-destructive"
    >
      {{ error }}
    </p>
  </fieldset>
</template>
