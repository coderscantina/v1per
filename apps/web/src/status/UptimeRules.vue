<script setup lang="ts">
import { t } from './messages'
import { rulesInFull } from './uptime'

/** "How uptime is counted": every rule behind the page's figures, closed until asked for. */
</script>

<template>
  <details
    data-testid="uptime-rules"
    class="group/rules"
  >
    <summary class="inline cursor-pointer list-none rounded-sm font-medium text-foreground underline-offset-4 hover:underline [&::-webkit-details-marker]:hidden">
      {{ t('uptime.how') }}
    </summary>
    <ul class="mt-2 flex max-w-prose list-disc flex-col gap-1 pl-5">
      <li
        v-for="rule in rulesInFull()"
        :key="rule"
      >{{ rule }}</li>
    </ul>
  </details>
</template>
