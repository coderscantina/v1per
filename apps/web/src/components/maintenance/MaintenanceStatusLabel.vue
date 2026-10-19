<script setup lang="ts">
import type { MaintenanceStatus } from '~/generated'
import { statusFill, statusLabels } from '~/lib/maintenance-form'
import { useI18n } from '~/plugins/i18n'

/** A maintenance's status as a coloured dot and its word; the word carries the meaning. */
defineProps<{ status: MaintenanceStatus }>()

const { t } = useI18n()
</script>

<template>
  <span
    class="inline-flex items-center gap-2 text-sm whitespace-nowrap text-foreground"
    :data-maintenance="status"
  >
    <span
      class="size-2 shrink-0 rounded-full"
      :class="statusFill[status]"
      aria-hidden="true"
    />
    {{ t(statusLabels[status]) }}
  </span>
</template>
