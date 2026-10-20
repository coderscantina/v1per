<script setup lang="ts">
// Adapted from Kit (f447e797, MIT): resources/js/components/FormField.vue, without field presence
// and the password reveal.
import FieldMessages from '~/components/FieldMessages.vue'
import type { HTMLAttributes } from 'vue'

import { Input } from '~/components/ui/input'
import { Label } from '~/components/ui/label'

const props = defineProps<{
  id: string
  label: string
  type?: string
  autocomplete?: string
  inputmode?: HTMLAttributes['inputmode']
  error?: string | null
  /** Hint under the field. Say what the rule is, not that there is one. */
  description?: string
  required?: boolean
  readonly?: boolean
  placeholder?: string
  /** Unit inside the field's end, such as "s", so the label can stay a plain noun. */
  suffix?: string
  maxlength?: number
}>()

const model = defineModel<string>({ default: '' })

/** The hint and the error are both announced, so the broken rule is never left unsaid. */
const describedBy = computed(() => {
  const ids = [
    props.suffix ? `${props.id}-suffix` : null,
    props.description ? `${props.id}-description` : null,
    props.error ? `${props.id}-error` : null,
  ]
  const present = ids.filter((id) => id !== null)
  return present.length > 0 ? present.join(' ') : undefined
})
</script>

<template>
  <div class="grid content-start gap-1.5">
    <Label :for="id">{{ label }}</Label>
    <div class="relative">
      <Input
        :id="id"
        v-model="model"
        :type="type ?? 'text'"
        :autocomplete="autocomplete"
        :inputmode="inputmode"
        :required="required"
        :readonly="readonly"
        :placeholder="placeholder"
        :maxlength="maxlength"
        :style="suffix ? { paddingRight: `calc(${suffix.length}ch + 1.5rem)` } : undefined"
        :aria-invalid="error ? 'true' : undefined"
        :aria-describedby="describedBy"
      />
      <span
        v-if="suffix"
        :id="`${id}-suffix`"
        class="pointer-events-none absolute inset-y-0 right-3 flex items-center text-sm text-muted"
      >
        {{ suffix }}
      </span>
    </div>
    <FieldMessages
      :id="id"
      :description="description"
      :error="error"
    />
  </div>
</template>
