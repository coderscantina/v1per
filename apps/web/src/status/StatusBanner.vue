<script setup lang="ts">
import type { PageStatus } from '~/generated'

import { formatClock } from './format'
import type { Connection } from './live'
import { locale, t, type MessageKey } from './messages'
import StatusIcon from './StatusIcon.vue'
import { toneIcon } from './tones'

/** The page's overall status, when it was read, and whether it follows live. */
defineProps<{ status: PageStatus; generatedAt: number; connection: Connection }>()

const headline: Record<PageStatus, MessageKey> = {
  operational: 'banner.operational',
  maintenance: 'banner.maintenance',
  degraded: 'banner.degraded',
  partial_outage: 'banner.partial_outage',
  major_outage: 'banner.major_outage',
}
</script>

<template>
  <section
    :data-tone="status"
    data-testid="page-status"
    class="flex flex-wrap items-center gap-x-4 gap-y-2 rounded-panel border border-[color-mix(in_srgb,var(--tone)_35%,var(--border))] bg-[color-mix(in_srgb,var(--tone)_8%,var(--card))] px-5 py-4 shadow-soft-sm"
  >
    <StatusIcon
      :name="toneIcon[status]"
      class="size-6 shrink-0 text-(--tone)"
    />
    <h2 class="min-w-0 flex-1 text-lg font-semibold text-primary">{{ t(headline[status]) }}</h2>
    <div class="flex w-full items-center gap-3 pl-10 text-sm text-muted sm:w-auto sm:pl-0">
      <span>{{ t('banner.updated', { time: formatClock(generatedAt, locale) }) }}</span>
      <span
        role="status"
        class="inline-flex items-center gap-1.5"
        data-testid="live"
      >
        <template v-if="connection !== 'connecting'">
          <span
            class="size-2 rounded-full"
            :class="connection === 'live' ? 'bg-success' : 'animate-pulse bg-warning'"
            aria-hidden="true"
          />
          {{ connection === 'live' ? t('live.live') : t('live.reconnecting') }}
        </template>
      </span>
    </div>
  </section>
</template>
