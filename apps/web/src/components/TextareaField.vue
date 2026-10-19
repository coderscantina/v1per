<script setup lang="ts">
import { Label } from '~/components/ui/label'
import { Textarea } from '~/components/ui/textarea'

/** A labelled multi-line field with a hint and an error under it, like `FormField`. */
const props = defineProps<{
  id: string
  label: string
  description?: string
  error?: string | null
  rows?: number
  /** Monospace, for templates and JSON. */
  code?: boolean
  placeholder?: string
  /** Another element that explains the field, such as a template help panel. */
  describedBy?: string
}>()

const model = defineModel<string>({ default: '' })

const describedBy = computed(() => {
  const ids = [
    props.description ? `${props.id}-description` : null,
    props.error ? `${props.id}-error` : null,
    props.describedBy ?? null,
  ]
  const present = ids.filter((id) => id !== null)
  return present.length > 0 ? present.join(' ') : undefined
})
</script>

<template>
  <div class="grid gap-1.5">
    <Label :for="id">{{ label }}</Label>
    <Textarea
      :id="id"
      v-model="model"
      :rows="rows ?? 3"
      :placeholder="placeholder"
      :class="code ? 'font-mono text-xs' : undefined"
      :aria-invalid="error ? 'true' : undefined"
      :aria-describedby="describedBy"
    />
    <p
      v-if="description"
      :id="`${id}-description`"
      class="text-xs text-muted"
    >
      {{ description }}
    </p>
    <p
      v-if="error"
      :id="`${id}-error`"
      class="text-xs font-medium text-destructive"
    >
      {{ error }}
    </p>
  </div>
</template>
