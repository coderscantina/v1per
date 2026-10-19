<script setup lang="ts">
import { Badge, type BadgeVariants } from '~/components/ui/badge'
import type { IncidentState } from '~/generated'
import { stateLabels } from '~/lib/incidents'
import { useI18n } from '~/plugins/i18n'

/** Where an incident stands, from investigating to resolved. */
defineProps<{ state: IncidentState }>()

const { t } = useI18n()
const variants: Record<IncidentState, BadgeVariants['variant']> = {
  investigating: 'destructive',
  identified: 'warning',
  monitoring: 'secondary',
  resolved: 'success',
}
</script>

<template>
  <Badge
    :variant="variants[state]"
    size="sm"
    :data-state="state"
  >
    {{ t(stateLabels[state]) }}
  </Badge>
</template>
