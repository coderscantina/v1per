<script setup lang="ts" generic="Value extends string">
import FieldMessages from '~/components/FieldMessages.vue'
import { Label } from '~/components/ui/label'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '~/components/ui/select'

/** A labelled choice from a short list, with a hint and an error under it like `FormField`. */
const props = defineProps<{
  id: string
  label: string
  options: ReadonlyArray<{ value: Value; label: string }>
  description?: string
  error?: string | null
}>()

const model = defineModel<Value>({ required: true })

const describedBy = computed(() => {
  const ids = [props.description ? `${props.id}-description` : null, props.error ? `${props.id}-error` : null]
  const present = ids.filter((id) => id !== null)
  return present.length > 0 ? present.join(' ') : undefined
})
const selected = computed(() => props.options.find((option) => option.value === model.value)?.label ?? '')
const choose = (value: unknown) => {
  const option = props.options.find((entry) => entry.value === value)
  if (option) model.value = option.value
}
</script>

<template>
  <div class="grid content-start gap-1.5">
    <Label :for="id">{{ label }}</Label>
    <Select
      :model-value="model"
      @update:model-value="choose"
    >
      <SelectTrigger
        :id="id"
        class="font-normal"
        :aria-invalid="error ? 'true' : undefined"
        :aria-describedby="describedBy"
      >
        <SelectValue>{{ selected }}</SelectValue>
      </SelectTrigger>
      <SelectContent>
        <SelectItem
          v-for="option in options"
          :key="option.value"
          :value="option.value"
        >
          {{ option.label }}
        </SelectItem>
      </SelectContent>
    </Select>
    <FieldMessages
      :id="id"
      :description="description"
      :error="error"
    />
  </div>
</template>
