<script setup lang="ts">
import { useI18n } from '~/plugins/i18n'

import { useStatusFacts, type StatusCardProps } from './status-facts'

/** One tinted line: the status, its message, and the figures that matter most at the end. */
const props = defineProps<StatusCardProps>()

const { t } = useI18n()
const facts = useStatusFacts(props)
const shown = computed(() => facts.value.windows.filter((window) => window.key === 'uptime_24h' || window.key === 'uptime_30d'))
</script>

<template>
  <section
    aria-labelledby="monitor-status"
    class="grid gap-3 rounded-panel border border-(--tone)/30 bg-(--tone)/8 px-4 py-2.5 text-primary [--muted:color-mix(in_oklab,var(--primary)_72%,transparent)]"
    :style="{ '--tone': facts.tone }"
  >
    <h2
      id="monitor-status"
      class="sr-only"
    >
      {{ t('monitors.detail.status') }}
    </h2>
    <div class="flex min-w-0 flex-wrap items-center gap-x-3 gap-y-1.5">
      <span
        class="size-2.5 shrink-0 rounded-full bg-(--tone)"
        aria-hidden="true"
      />
      <span
        class="font-semibold"
        :data-status="presence"
      >{{ facts.label }}</span>
      <span
        v-if="facts.held"
        class="text-sm text-muted tabular-nums"
      >{{ facts.held }}</span>
      <p
        class="min-w-0 flex-1 basis-24 truncate text-sm"
        :class="facts.message === null ? 'text-muted' : 'select-text'"
        :title="facts.message ?? undefined"
        data-testid="monitor-message"
      >
        {{ facts.message ?? t('monitors.detail.noBeat') }}
      </p>
      <dl class="flex flex-wrap items-baseline gap-x-4 gap-y-1 text-sm tabular-nums">
        <div
          v-for="window in shown"
          :key="window.key"
          class="flex gap-1.5"
          :title="window.long"
        >
          <dt class="text-muted">{{ window.label }}</dt>
          <dd class="font-semibold">{{ window.value ?? '…' }}</dd>
        </div>
        <div
          v-if="facts.latency"
          class="flex gap-1.5"
        >
          <dt class="sr-only">{{ t('monitors.detail.latency') }}</dt>
          <dd class="font-semibold">{{ facts.latency }}</dd>
        </div>
        <div
          v-if="facts.lastCheck"
          class="flex gap-1.5"
        >
          <dt class="sr-only">{{ t('monitors.detail.lastCheck') }}</dt>
          <dd
            class="text-muted"
            :title="facts.lastCheck.title"
          >
            {{ facts.lastCheck.text }}
          </dd>
        </div>
      </dl>
    </div>
    <slot />
  </section>
</template>
