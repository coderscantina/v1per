<script setup lang="ts">
import type { RouteLocationRaw } from 'vue-router'

import Icon from '~/components/Icon.vue'

/**
 * One block of the dashboard's summary column: a small heading with a muted count, an optional
 * link to the full list, and the content below. The heading names the region.
 */
defineProps<{
  id: string
  title: string
  meta?: string
  to?: RouteLocationRaw
  linkLabel?: string
}>()
</script>

<template>
  <section
    :aria-labelledby="id"
    class="grid content-start gap-2.5"
  >
    <header class="flex items-center gap-1.5">
      <h2
        :id="id"
        class="text-[0.78125rem] font-semibold text-primary"
      >
        {{ title }}
      </h2>
      <span
        v-if="meta"
        class="text-[0.78125rem] text-muted tabular-nums"
      >{{ meta }}</span>
      <RouterLink
        v-if="to && linkLabel"
        :to="to"
        class="ml-auto inline-flex items-center gap-1 rounded-sm text-xs text-muted transition-colors hover:text-primary focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring"
      >
        {{ linkLabel }}
        <Icon
          name="lucide:arrow-right"
          size="12"
          aria-hidden="true"
        />
      </RouterLink>
    </header>
    <slot />
  </section>
</template>
