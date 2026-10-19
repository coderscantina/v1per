<script setup lang="ts">
import { iconPaths, type IconName } from './icons'

/** A Lucide icon from `icons.ts`. Decorative: a word always goes with it. */
defineProps<{ name: IconName }>()
</script>

<template>
  <svg
    xmlns="http://www.w3.org/2000/svg"
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    stroke-width="2"
    stroke-linecap="round"
    stroke-linejoin="round"
    aria-hidden="true"
    focusable="false"
    v-html="iconPaths[name]"
  />
</template>
