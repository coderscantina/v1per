<script setup lang="ts">
import Icon from '~/components/Icon.vue'
import StatFigure from '~/components/StatFigure.vue'
import { useI18n } from '~/plugins/i18n'

import { useStatusFacts, type StatusCardProps } from './status-facts'

/**
 * The default design: a tinted block with the status and its message, the latest check, and
 * uptime over Kuma's four windows as figures.
 */
const props = defineProps<StatusCardProps>()

const { t } = useI18n()
const facts = useStatusFacts(props)
</script>

<template>
  <section
    aria-labelledby="monitor-status"
    class="grid gap-5 rounded-panel border border-(--tone)/30 bg-(--tone)/8 px-5 py-4.5 text-primary [--muted:color-mix(in_oklab,var(--primary)_72%,transparent)]"
    :style="{ '--tone': facts.tone }"
  >
    <h2
      id="monitor-status"
      class="sr-only"
    >
      {{ t('monitors.detail.status') }}
    </h2>
    <div class="flex flex-wrap items-start justify-between gap-x-10 gap-y-4">
      <div class="flex min-w-0 flex-1 basis-72 items-start gap-3">
        <Icon
          :name="facts.icon"
          size="24"
          class="mt-1 shrink-0 text-(--tone)"
          aria-hidden="true"
        />
        <div class="grid min-w-0 gap-1">
          <p class="flex flex-wrap items-baseline gap-x-2.5">
            <span
              class="text-[1.625rem]/[2rem] font-semibold tracking-tight"
              :data-status="presence"
            >{{ facts.label }}</span>
            <span
              v-if="facts.held"
              class="text-sm text-muted tabular-nums"
            >{{ facts.held }}</span>
          </p>
          <p
            class="text-sm break-words"
            :class="facts.message === null ? 'text-muted' : 'select-text'"
            data-testid="monitor-message"
          >
            {{ facts.message ?? t('monitors.detail.noBeat') }}
          </p>
        </div>
      </div>
      <dl
        v-if="facts.lastCheck"
        class="flex gap-8 max-sm:pl-9"
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
    <div class="grid gap-2 border-t border-(--tone)/20 pt-4">
      <h3 class="text-[0.8125rem] font-medium text-muted">{{ t('monitors.detail.uptime') }}</h3>
      <dl class="grid grid-cols-2 gap-x-6 gap-y-3 sm:grid-cols-4">
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
    <slot />
  </section>
</template>
