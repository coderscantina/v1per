<script setup lang="ts">
import StatFigure from '~/components/StatFigure.vue'
import { useI18n } from '~/plugins/i18n'

import { useStatusFacts, type StatusCardProps } from './status-facts'

/** The status in large type in its colour, with a live dot, and the uptime windows as big figures. */
const props = defineProps<StatusCardProps>()

const { t } = useI18n()
const facts = useStatusFacts(props)
</script>

<template>
  <section
    aria-labelledby="monitor-status"
    class="grid gap-6 rounded-panel border border-hairline bg-card px-6 py-5 lg:grid-cols-[minmax(0,1fr)_auto]"
    :style="{ '--tone': facts.tone }"
  >
    <div class="grid min-w-0 content-start gap-2">
      <h2
        id="monitor-status"
        class="flex items-center gap-2 text-[0.8125rem] text-muted"
      >
        <span
          class="relative flex size-2.5"
          aria-hidden="true"
        >
          <span
            v-if="presence === 'up' || presence === 'down'"
            class="absolute inline-flex size-full animate-ping rounded-full bg-(--tone) opacity-60 motion-reduce:hidden"
          />
          <span class="relative inline-flex size-2.5 rounded-full bg-(--tone)" />
        </span>
        {{ t('monitors.detail.status') }}
      </h2>
      <p class="flex flex-wrap items-baseline gap-x-3">
        <span
          class="text-[2.75rem]/[3.125rem] font-semibold tracking-tight text-(--tone)"
          :data-status="presence"
        >{{ facts.label }}</span>
        <span
          v-if="facts.held"
          class="text-base text-muted tabular-nums"
        >{{ facts.held }}</span>
      </p>
      <p
        class="text-sm break-words"
        :class="facts.message === null ? 'text-muted' : 'text-foreground select-text'"
        data-testid="monitor-message"
      >
        {{ facts.message ?? t('monitors.detail.noBeat') }}
      </p>
      <dl
        v-if="facts.lastCheck"
        class="mt-2 flex gap-8"
      >
        <StatFigure
          v-if="facts.latency"
          :label="t('monitors.detail.latency')"
          :value="facts.latency"
          size="lg"
          label-below
        />
        <StatFigure
          :label="t('monitors.detail.lastCheck')"
          :value="facts.lastCheck.text"
          :title="facts.lastCheck.title"
          size="lg"
          label-below
        />
      </dl>
    </div>
    <div class="grid content-center gap-2 lg:border-l lg:border-hairline lg:pl-8">
      <h3 class="text-[0.8125rem] font-medium text-muted">{{ t('monitors.detail.uptime') }}</h3>
      <dl class="grid grid-cols-2 gap-x-10 gap-y-4 sm:grid-cols-4 lg:grid-cols-2">
        <StatFigure
          v-for="window in facts.windows"
          :key="window.key"
          :label="window.label"
          :value="window.value"
          :title="window.long"
          size="xl"
          label-below
        />
      </dl>
    </div>
    <div
      v-if="$slots.default"
      class="lg:col-span-full"
    >
      <slot />
    </div>
  </section>
</template>
