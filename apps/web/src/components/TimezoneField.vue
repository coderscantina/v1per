<script setup lang="ts">
import { Input } from '~/components/ui/input'
import { Label } from '~/components/ui/label'

/**
 * An IANA timezone, typed with suggestions from the browser's list (`Europe/Vienna`). The
 * server checks the name; an empty value means the default the hint names.
 */
const props = defineProps<{
  id: string
  label: string
  description?: string
  error?: string | null
  placeholder?: string
}>()

const model = defineModel<string>({ default: '' })

const zones = Intl.supportedValuesOf('timeZone')
const describedBy = computed(() => {
  const ids = [props.description ? `${props.id}-description` : null, props.error ? `${props.id}-error` : null]
  const present = ids.filter((id) => id !== null)
  return present.length > 0 ? present.join(' ') : undefined
})
</script>

<template>
  <div class="grid gap-1.5">
    <Label :for="id">{{ label }}</Label>
    <Input
      :id="id"
      v-model="model"
      :list="`${id}-zones`"
      autocomplete="off"
      spellcheck="false"
      :placeholder="placeholder"
      :aria-invalid="error ? 'true' : undefined"
      :aria-describedby="describedBy"
    />
    <datalist :id="`${id}-zones`">
      <option
        v-for="zone in zones"
        :key="zone"
        :value="zone"
      />
    </datalist>
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
