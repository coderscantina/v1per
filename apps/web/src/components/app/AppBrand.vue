<script setup lang="ts">
// Adapted from Kit (f447e797, MIT): resources/js/components/app/AppBrand.vue. The name is the
// installation's, from the runtime config.
import { runtimeConfig } from '~/services/runtime-config'

defineProps<{
  /** Rail mode: the mark alone, with the name kept for screen readers. */
  compact?: boolean
}>()
</script>

<template>
  <RouterLink
    :to="{ name: 'home' }"
    class="flex h-8 min-w-0 items-center gap-2 rounded-md px-2.5 text-sm font-semibold text-sidebar-accent-foreground transition-colors duration-200 ease-butter hover:bg-sidebar-accent"
    :class="compact && 'w-8 justify-center px-0'"
  >
    <svg
      class="h-3 w-auto shrink-0 text-accent"
      viewBox="0 0 311.52 143.97"
      fill="currentColor"
      aria-hidden="true"
    >
      <path
        d="M111.97 38.61l-13.85 24.84-15.92 28.64L29.01.02 0 .07l30.14 52.36 29.75 51.78 22.64 39.77 58-105.4-22.47-.12c-2.12-.01-4.07-.27-6.1.15Z"
      />
      <path d="M119.89 23.99h30.9v114.04l24.84-.03.02-137.97h-42.25z" />
      <path
        d="M307.83 28.42C300.83 11.76 285.18.81 266.97.06L191.35 0v23.97l74.12.06c13.71.84 23.27 12.78 20.76 26.36-1.74 9.38-9.52 17.99-19.63 18.02l-75.26.2v69.38l25.16-.08-.04-45.37 51.62-.05c13.33-.01 25.47-6.04 33.6-16.49 10.44-13.41 12.69-31.99 6.13-47.61Z"
      />
    </svg>
    <span
      class="truncate"
      :class="compact && 'sr-only'"
    >
      {{ runtimeConfig.instance_name }}
    </span>
  </RouterLink>
</template>
