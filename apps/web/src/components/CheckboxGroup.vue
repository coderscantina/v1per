<script setup lang="ts" generic="T extends string">
// Adapted from holocron (84fc54ec): apps/web/src/components/CheckboxGroup.vue. One line per
// option: the name, and a short muted meta on the right. "Select all" shows only on longer lists.
import { Checkbox } from '~/components/ui/checkbox'
import { useI18n } from '~/plugins/i18n'

const props = defineProps<{
  legend: string
  /** `hint` is one or two words of meta, such as the kind or why the option is unavailable. */
  options: { value: T; label: string; hint?: string; disabled?: boolean }[]
  error?: string | null
}>()
const model = defineModel<T[]>({ required: true })

const { t } = useI18n()
const id = useId()

/** Below this many options a "Select all" costs more attention than the clicks it saves. */
const SELECT_ALL_FROM = 5

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
    class="grid gap-1.5"
    :aria-describedby="error ? `${id}-error` : undefined"
  >
    <legend class="mb-1.5 text-sm font-medium text-primary">{{ legend }}</legend>
    <div class="divide-y divide-divider overflow-hidden rounded-control border border-hairline">
      <label
        v-if="available.length >= SELECT_ALL_FROM"
        class="flex h-10 cursor-pointer items-center gap-3 px-3 text-sm text-muted transition-colors hover:bg-fill hover:text-primary"
      >
        <Checkbox
          :model-value="all"
          @update:model-value="toggleAll"
        />
        {{ t('actions.selectAll') }}
      </label>
      <label
        v-for="option in options"
        :key="option.value"
        class="flex h-10 min-w-0 items-center gap-3 px-3 text-sm transition-colors"
        :class="option.disabled ? 'text-muted' : 'cursor-pointer text-primary hover:bg-fill'"
      >
        <Checkbox
          :model-value="model.includes(option.value)"
          :disabled="option.disabled"
          @update:model-value="(on) => toggle(option.value, on)"
        />
        <span class="min-w-0 flex-1 truncate">{{ option.label }}</span>
        <span
          v-if="option.hint"
          class="max-w-1/2 shrink-0 truncate text-muted"
          :title="option.hint"
        >
          {{ option.hint }}
        </span>
      </label>
    </div>
    <p
      v-if="error"
      :id="`${id}-error`"
      class="text-sm font-medium text-destructive"
    >
      {{ error }}
    </p>
  </fieldset>
</template>
