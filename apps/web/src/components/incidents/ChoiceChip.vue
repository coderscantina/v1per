<script setup lang="ts" generic="Value extends string">
import Icon from '~/components/Icon.vue'
import { Button } from '~/components/ui/button'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuTrigger,
} from '~/components/ui/dropdown-menu'

/**
 * One choice as a compact chip: a coloured dot and the chosen word, opening a menu of the
 * options. `placeholder` shows while nothing is chosen. `label` names the chip for assistive
 * technology, followed by the choice.
 */
const props = defineProps<{
  label: string
  options: readonly { value: Value; label: string; fill: string }[]
  placeholder?: string
}>()
const model = defineModel<Value | undefined>({ required: true })

const chosen = computed(() => props.options.find((option) => option.value === model.value))
const choose = (value: unknown) => {
  const option = props.options.find((entry) => entry.value === value)
  if (option) model.value = option.value
}
</script>

<template>
  <DropdownMenu>
    <DropdownMenuTrigger as-child>
      <Button
        type="button"
        size="sm"
        variant="fill"
        :aria-label="`${label}: ${chosen?.label ?? placeholder ?? ''}`"
        :data-chip="chosen?.value"
      >
        <span
          class="size-2 shrink-0 rounded-full"
          :class="chosen?.fill ?? 'bg-status-none'"
          aria-hidden="true"
        />
        {{ chosen?.label ?? placeholder }}
        <Icon
          name="lucide:chevron-down"
          class="text-muted"
        />
      </Button>
    </DropdownMenuTrigger>
    <DropdownMenuContent
      align="start"
      class="w-48"
    >
      <DropdownMenuRadioGroup
        :model-value="model"
        @update:model-value="choose"
      >
        <DropdownMenuRadioItem
          v-for="option in options"
          :key="option.value"
          :value="option.value"
        >
          <span class="inline-flex items-center gap-2">
            <span
              class="size-2 shrink-0 rounded-full"
              :class="option.fill"
              aria-hidden="true"
            />
            {{ option.label }}
          </span>
        </DropdownMenuRadioItem>
      </DropdownMenuRadioGroup>
    </DropdownMenuContent>
  </DropdownMenu>
</template>
