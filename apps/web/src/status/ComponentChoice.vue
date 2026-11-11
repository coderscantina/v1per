<script setup lang="ts">
import { t } from './messages'

/**
 * What a subscription follows: the whole page, or some of its components. An empty selection
 * means the whole page, as on the wire.
 */
const props = defineProps<{ components: { id: string; name: string }[] }>()
const chosen = defineModel<string[]>({ required: true })

const some = ref(chosen.value.length > 0)

function everything() {
  some.value = false
  chosen.value = []
}

function toggle(id: string, on: boolean) {
  // Kept in page order, whatever order they were ticked in.
  chosen.value = props.components.map((component) => component.id).filter((other) => (other === id ? on : chosen.value.includes(other)))
}
</script>

<template>
  <fieldset class="flex flex-col gap-2">
    <legend class="mb-2 text-sm font-medium text-primary">{{ t('subscribe.about') }}</legend>
    <label class="flex items-center gap-2 text-sm text-primary">
      <input
        type="radio"
        name="subscribe-scope"
        class="size-4 accent-(--accent)"
        :checked="!some"
        @change="everything"
      >
      {{ t('subscribe.everything') }}
    </label>
    <label class="flex items-center gap-2 text-sm text-primary">
      <input
        type="radio"
        name="subscribe-scope"
        class="size-4 accent-(--accent)"
        :checked="some"
        @change="some = true"
      >
      {{ t('subscribe.some') }}
    </label>
    <ul
      v-if="some"
      class="ml-6 flex max-h-48 flex-col gap-1.5 overflow-y-auto"
    >
      <li
        v-for="component in components"
        :key="component.id"
      >
        <label class="flex items-center gap-2 text-sm text-foreground">
          <input
            type="checkbox"
            class="size-4 accent-(--accent)"
            :checked="chosen.includes(component.id)"
            @change="toggle(component.id, ($event.target as HTMLInputElement).checked)"
          >
          {{ component.name }}
        </label>
      </li>
    </ul>
  </fieldset>
</template>
