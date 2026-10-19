<script lang="ts">
import { addCollection, Icon as IconifyIcon } from '@iconify/vue/offline'

import { lucide } from '~/lib/icons'

addCollection(lucide)
</script>

<script setup lang="ts">
import type { HTMLAttributes } from 'vue'

import type { IconName } from '~/lib/icons'

/**
 * Renders a Lucide icon by its prefixed name. The names are typed, so a field or a map that
 * holds an icon takes `IconName` and a misspelt one fails the typecheck. Only the icons the
 * sources name are bundled (`~/lib/icons.ts`); to add one, use its name and regenerate the subset with
 * `bunx vitest run -u src/lib/icons.test.ts`. Nothing is fetched at runtime.
 */
const props = defineProps<{
  name?: IconName | null
  size?: string | number
  class?: HTMLAttributes['class']
}>()

const iconSize = computed(() => {
  if (!props.size) return '1rem'
  return typeof props.size === 'number' ? `${props.size}px` : props.size
})
</script>

<template>
  <IconifyIcon
    v-if="name"
    :icon="name"
    :width="iconSize"
    :height="iconSize"
    :class="props.class"
  />
</template>
