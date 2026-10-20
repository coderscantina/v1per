<script setup lang="ts">
import Icon from '~/components/Icon.vue'
import StatFigure from '~/components/StatFigure.vue'
import { useI18n } from '~/plugins/i18n'

import { useStatusFacts, type StatusCardProps } from './status-facts'

/**
 * Three rings, 24 hours outside to 30 days inside, green for the share that was up. Any downtime
 * shows as a red notch at the ring's end, wide enough to see even at 99.99 %. The status sits
 * inside. Pointing at a ring or its figure, or tapping either on a touch screen, picks out both.
 */
const props = defineProps<StatusCardProps>()

const { t } = useI18n()
const facts = useStatusFacts(props)

const STROKE = 9
const RADII = [78, 64, 50] as const
/** Downtime is at least this share of a ring, so a short outage stays visible. */
const NOTCH_MIN = 0.025
const GAP = 0.008

const rings = computed(() =>
  facts.value.windows.slice(0, 3).map((window, index) => {
    const radius = RADII[index] ?? 50
    const length = 2 * Math.PI * radius
    const ratio = window.ratio
    const down = ratio === undefined || ratio >= 1 ? 0 : Math.max(1 - ratio, NOTCH_MIN)
    const up = ratio === undefined ? 0 : (1 - down - (down > 0 ? GAP * 2 : 0)) * length
    return {
      ...window,
      index,
      radius,
      up: `${up} ${length}`,
      down: down > 0 ? { dash: `${down * length} ${length}`, offset: -(up + GAP * length) } : null,
    }
  })
)
const year = computed(() => facts.value.windows[3])

/** The ring picked out with its figure, by pointer or by tap. */
const active = ref<number | null>(null)
const point = (index: number | null, event: PointerEvent) => {
  if (event.pointerType === 'mouse') active.value = index
}
const tap = (index: number) => (active.value = active.value === index ? null : index)
const dimmed = (index: number) => active.value !== null && active.value !== index

/** An HTTP code's tone: success, redirect, or failure. */
const codeTone = (code: number) => (code < 300 ? 'var(--status-up)' : code < 400 ? 'var(--status-maintenance)' : 'var(--status-down)')
</script>

<template>
  <section
    aria-labelledby="monitor-status"
    class="grid items-center gap-x-10 gap-y-6 rounded-panel border border-hairline bg-card px-6 py-5 sm:grid-cols-[auto_minmax(0,1fr)]"
    :style="{ '--tone': facts.tone }"
  >
    <h2
      id="monitor-status"
      class="sr-only"
    >
      {{ t('monitors.detail.status') }}
    </h2>
    <div class="relative mx-auto size-48 shrink-0">
      <svg
        viewBox="0 0 176 176"
        class="size-full -rotate-90"
        aria-hidden="true"
      >
        <g
          v-for="ring in rings"
          :key="ring.key"
          class="cursor-pointer transition-opacity duration-100"
          :class="dimmed(ring.index) && 'opacity-25'"
          @pointerenter="point(ring.index, $event)"
          @pointerleave="point(null, $event)"
          @click="tap(ring.index)"
        >
          <circle
            cx="88"
            cy="88"
            :r="ring.radius"
            fill="none"
            :stroke-width="STROKE"
            class="stroke-fill-strong"
          />
          <circle
            cx="88"
            cy="88"
            :r="ring.radius"
            fill="none"
            :stroke-width="STROKE"
            class="stroke-status-up transition-[stroke-dasharray] duration-300 ease-out motion-reduce:transition-none"
            :stroke-dasharray="ring.up"
          />
          <circle
            v-if="ring.down"
            cx="88"
            cy="88"
            :r="ring.radius"
            fill="none"
            :stroke-width="STROKE"
            class="stroke-status-down"
            :stroke-dasharray="ring.down.dash"
            :stroke-dashoffset="ring.down.offset"
          />
        </g>
      </svg>
      <div class="pointer-events-none absolute inset-0 grid place-content-center justify-items-center gap-0.5 text-center">
        <Icon
          :name="facts.icon"
          size="20"
          class="text-(--tone)"
          aria-hidden="true"
        />
        <span
          class="text-xl/7 font-semibold text-primary"
          :data-status="presence"
        >{{ facts.label }}</span>
        <span
          v-if="facts.held"
          class="text-xs text-muted tabular-nums"
        >{{ facts.held }}</span>
      </div>
    </div>
    <div class="@container grid min-w-0 content-center gap-6">
      <p
        v-if="facts.response"
        class="flex min-w-0 items-center gap-2.5"
        data-testid="monitor-message"
        :title="facts.message ?? undefined"
      >
        <span
          class="rounded-md bg-(--code)/14 px-2 py-0.5 text-base font-semibold text-(--code) tabular-nums"
          :style="{ '--code': codeTone(facts.response.code) }"
        >{{ facts.response.code }}</span>
        <span class="truncate text-base font-medium text-primary select-text">{{ facts.response.reason }}</span>
      </p>
      <p
        v-else
        class="flex min-w-0 items-start gap-2 text-sm break-words"
        :class="facts.message === null ? 'text-muted' : 'text-foreground select-text'"
        data-testid="monitor-message"
      >
        <Icon
          v-if="facts.message !== null"
          name="lucide:message-circle"
          size="14"
          class="mt-0.75 shrink-0 text-muted"
          aria-hidden="true"
        />
        {{ facts.message ?? t('monitors.detail.noBeat') }}
      </p>
      <dl
        class="grid grid-cols-2 gap-x-8 gap-y-5 @lg:grid-cols-4"
        :aria-label="t('monitors.detail.uptime')"
      >
        <StatFigure
          v-for="ring in rings"
          :key="ring.key"
          :label="ring.label"
          :value="ring.value"
          :title="ring.long"
          size="xl"
          label-below
          class="cursor-pointer transition-opacity duration-100"
          :class="dimmed(ring.index) && 'opacity-35'"
          @pointerenter="point(ring.index, $event)"
          @pointerleave="point(null, $event)"
          @click="tap(ring.index)"
        >
          <template #label>
            <svg
              viewBox="0 0 16 16"
              class="size-3.5 shrink-0"
              aria-hidden="true"
            >
              <circle
                v-for="(radius, index) in [7, 4.75, 2.5]"
                :key="radius"
                cx="8"
                cy="8"
                :r="radius"
                fill="none"
                stroke-width="1.5"
                :class="index === ring.index ? 'stroke-status-up' : 'stroke-fill-strong'"
              />
            </svg>
            <span class="truncate">{{ ring.label }}</span>
          </template>
        </StatFigure>
        <StatFigure
          v-if="year"
          :label="year.label"
          :value="year.value"
          :title="year.long"
          size="xl"
          label-below
          class="transition-opacity duration-100"
          :class="active !== null && 'opacity-35'"
        />
      </dl>
      <dl
        v-if="facts.lastCheck"
        class="flex gap-10 border-t border-hairline pt-4"
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
      <slot />
    </div>
  </section>
</template>
