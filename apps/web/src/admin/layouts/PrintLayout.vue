<script setup lang="ts">
import { useColorMode } from '~/composables/useColorMode'

/**
 * A page meant for paper: no shell, light colors whatever the reader's mode, and on screen a
 * white sheet at about the width of A4 on the canvas.
 */
useColorMode().forceInScope('light')
</script>

<template>
  <main
    id="main-content"
    tabindex="-1"
    class="min-h-svh bg-surface px-4 py-6 focus:outline-none sm:px-8 sm:py-10 print:bg-white print:p-0"
  >
    <slot />
  </main>
</template>
