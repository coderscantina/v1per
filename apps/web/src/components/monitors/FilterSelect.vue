<script setup lang="ts">
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '~/components/ui/select'

/** A toolbar filter: a compact select named by `label`, where the empty value means all. */
const props = defineProps<{
  label: string
  allLabel: string
  options: ReadonlyArray<{ value: string; label: string }>
}>()

const model = defineModel<string>({ required: true })
const ALL = '*'
const selected = computed(() => props.options.find((option) => option.value === model.value)?.label ?? props.allLabel)
</script>

<template>
  <Select
    :model-value="model === '' ? ALL : model"
    @update:model-value="(value) => (model = value === ALL || typeof value !== 'string' ? '' : value)"
  >
    <SelectTrigger
      class="h-9 w-auto min-w-36 gap-2 font-normal max-sm:flex-1"
      :aria-label="label"
    >
      <SelectValue>{{ selected }}</SelectValue>
    </SelectTrigger>
    <SelectContent>
      <SelectItem :value="ALL">{{ allLabel }}</SelectItem>
      <SelectItem
        v-for="option in options"
        :key="option.value"
        :value="option.value"
      >
        {{ option.label }}
      </SelectItem>
    </SelectContent>
  </Select>
</template>
