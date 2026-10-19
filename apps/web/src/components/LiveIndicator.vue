<script setup lang="ts">
import type { LiveStatus } from '~/composables/useLive'
import { useI18n, type MessageKey } from '~/plugins/i18n'

/** The state of a live stream in words, with a dot as the second signal. */
defineProps<{ status: LiveStatus }>()

const { t } = useI18n()

const labels: Record<LiveStatus, MessageKey> = {
  current: 'live.current',
  stale: 'live.stale',
  connecting: 'live.connecting',
  resyncing: 'live.resyncing',
  disconnected: 'live.disconnected',
}

const dots: Record<LiveStatus, string> = {
  current: 'bg-success',
  stale: 'bg-success',
  connecting: 'bg-faint animate-pulse',
  resyncing: 'bg-warning animate-pulse',
  disconnected: 'bg-destructive',
}
</script>

<template>
  <span
    role="status"
    class="inline-flex items-center gap-2 text-sm text-muted"
  >
    <span
      class="size-2 shrink-0 rounded-full"
      :class="dots[status]"
      aria-hidden="true"
    />
    {{ t(labels[status]) }}
  </span>
</template>
