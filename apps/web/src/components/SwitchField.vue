<script setup lang="ts">
import { Label } from '~/components/ui/label'
import { Switch } from '~/components/ui/switch'

/** An on/off setting: the switch, its label beside it and an optional hint below. */
const props = defineProps<{
  id: string
  label: string
  description?: string
  disabled?: boolean
}>()

const model = defineModel<boolean>({ required: true })
</script>

<template>
  <div class="grid grid-cols-[auto_1fr] items-start gap-x-3 gap-y-1">
    <Switch
      :id="id"
      v-model="model"
      :disabled="disabled"
      class="mt-px"
      :aria-describedby="props.description ? `${id}-description` : undefined"
    />
    <Label
      :for="id"
      class="leading-5"
    >
      {{ label }}
    </Label>
    <p
      v-if="description"
      :id="`${id}-description`"
      class="col-start-2 text-xs text-muted"
    >
      {{ description }}
    </p>
  </div>
</template>
