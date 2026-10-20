<script setup lang="ts">
import RailSection from '~/components/observe/RailSection.vue'
import { Badge } from '~/components/ui/badge'
import type { Monitor } from '~/generated'
import { useI18n } from '~/plugins/i18n'

/**
 * How the monitor is set up, for the summary column: what decides its status beyond the check
 * itself, its tags and its description. The type, target and interval are in the page header.
 */
const props = defineProps<{ monitor: Monitor; timed: boolean }>()

const { t } = useI18n()

const rows = computed(() => {
  const { monitor } = props
  return [
    { key: 'retries', label: t('monitors.detail.retries'), value: String(monitor.max_retries) },
    props.timed && monitor.settings.type !== 'push'
      ? { key: 'timeout', label: t('monitors.detail.timeout'), value: t('monitors.detail.seconds', { seconds: monitor.timeout }) }
      : null,
    monitor.upside_down ? { key: 'upside', label: t('monitors.detail.upsideDown'), value: t('monitors.detail.upsideDownOn') } : null,
  ].filter((row) => row !== null)
})
</script>

<template>
  <RailSection
    id="monitor-details"
    :title="t('monitors.detail.details')"
  >
    <dl class="grid gap-2 text-[0.8125rem]">
      <div
        v-for="row in rows"
        :key="row.key"
        class="flex items-baseline justify-between gap-4"
      >
        <dt class="text-muted">{{ row.label }}</dt>
        <dd class="font-medium text-primary tabular-nums">{{ row.value }}</dd>
      </div>
      <div
        v-if="monitor.tags.length > 0"
        class="flex items-baseline justify-between gap-4"
      >
        <dt class="text-muted">{{ t('monitors.detail.tags') }}</dt>
        <dd class="flex flex-wrap justify-end gap-1">
          <Badge
            v-for="tag in monitor.tags"
            :key="tag"
            variant="surface"
            size="sm"
          >
            {{ tag }}
          </Badge>
        </dd>
      </div>
    </dl>
    <p
      v-if="monitor.description"
      class="text-[0.8125rem] break-words whitespace-pre-line text-foreground select-text"
    >
      {{ monitor.description }}
    </p>
  </RailSection>
</template>
