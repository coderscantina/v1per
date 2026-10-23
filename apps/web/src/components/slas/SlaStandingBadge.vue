<script setup lang="ts">
import { Badge } from '~/components/ui/badge'
import { standingLabels, standingTones, type SlaStanding } from '~/lib/sla-period'
import { useI18n } from '~/plugins/i18n'

/** How a commitment or an incident clock stands. Only a miss is red; no data stays neutral. */
defineProps<{ standing: SlaStanding }>()

const { t } = useI18n()
</script>

<template>
  <Badge
    :variant="standingTones[standing]"
    size="sm"
    class="whitespace-nowrap"
    :data-standing="standing"
  >
    {{ t(standingLabels[standing]) }}
  </Badge>
</template>
