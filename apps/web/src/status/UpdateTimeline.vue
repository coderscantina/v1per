<script setup lang="ts">
import type { PublicIncidentUpdate } from '~/generated'

import { formatTime } from './format'
import { locale, t } from './messages'
import { stateLabel } from './tones'

/** An incident's updates, newest first. */
defineProps<{ updates: PublicIncidentUpdate[] }>()
</script>

<template>
  <ol class="flex flex-col gap-3 border-l-2 border-border pl-4">
    <li
      v-for="(update, index) in updates"
      :key="index"
      data-update
      class="flex flex-col gap-0.5"
    >
      <p class="text-sm text-muted">
        <span class="font-medium text-primary">{{ t(stateLabel[update.state]) }}</span>
        · <time :datetime="new Date(update.created_at).toISOString()">{{ formatTime(update.created_at, locale) }}</time>
      </p>
      <p
        v-if="update.message"
        class="whitespace-pre-line text-foreground"
      >{{ update.message }}</p>
    </li>
  </ol>
</template>
