<script setup lang="ts">
import type { PageStatus } from '~/generated'

import { formatClock } from './format'
import { locale, t, type MessageKey } from './messages'
import StatusIcon from './StatusIcon.vue'
import { toneIcon } from './tones'

/** The page's overall status and when it was read, centered above everything else. */
defineProps<{ status: PageStatus; generatedAt: number }>()

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
    class="flex flex-col items-center gap-2 py-2 text-center sm:py-4"
  >
    <StatusIcon
      :name="toneIcon[status]"
      class="mb-1 size-12 text-(--tone)"
    />
    <h2 class="text-2xl font-semibold tracking-tight text-primary">{{ t(headline[status]) }}</h2>
    <p class="text-sm text-muted">{{ t('banner.updated', { time: formatClock(generatedAt, locale) }) }}</p>
  </section>
</template>
