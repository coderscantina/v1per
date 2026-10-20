<script setup lang="ts">
/**
 * The hint and the error under a field, at the body size: muted color sets them apart, not a
 * smaller font. Ids follow `<field>-description` and `<field>-error` for `aria-describedby`.
 */
defineProps<{
  id: string
  description?: string
  error?: string | null
}>()
</script>

<template>
  <p
    v-if="description"
    :id="`${id}-description`"
    class="text-sm text-pretty text-muted"
  >
    {{ description }}
  </p>
  <p
    v-if="error"
    :id="`${id}-error`"
    class="text-sm font-medium text-destructive"
  >
    {{ error }}
  </p>
</template>
