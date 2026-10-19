<script setup lang="ts">
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '~/components/ui/select'
import type { Impact } from '~/generated'
import { impactFill, impactLabels, impacts } from '~/lib/incidents'
import { useI18n } from '~/plugins/i18n'

/** A compact impact choice for one target, named by `label`. `undefined` reads as unchanged. */
const props = defineProps<{ label: string; placeholder?: string }>()
const model = defineModel<Impact | undefined>({ required: true })

const { t } = useI18n()
const choose = (value: unknown) => {
  const impact = impacts.find((entry) => entry === value)
  if (impact) model.value = impact
}
</script>

<template>
  <Select
    :model-value="model"
    @update:model-value="choose"
  >
    <SelectTrigger
      class="h-9 w-full min-w-40 gap-2 font-normal sm:w-48"
      :aria-label="props.label"
    >
      <SelectValue :placeholder="placeholder">
        <span
          v-if="model"
          class="inline-flex items-center gap-2"
        >
          <span
            class="size-2 shrink-0 rounded-full"
            :class="impactFill[model]"
            aria-hidden="true"
          />
          {{ t(impactLabels[model]) }}
        </span>
      </SelectValue>
    </SelectTrigger>
    <SelectContent>
      <SelectItem
        v-for="impact in impacts"
        :key="impact"
        :value="impact"
      >
        <span class="inline-flex items-center gap-2">
          <span
            class="size-2 shrink-0 rounded-full"
            :class="impactFill[impact]"
            aria-hidden="true"
          />
          {{ t(impactLabels[impact]) }}
        </span>
      </SelectItem>
    </SelectContent>
  </Select>
</template>
