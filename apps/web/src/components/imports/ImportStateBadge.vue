<script setup lang="ts">
import { Badge } from '~/components/ui/badge'
import type { ImportState } from '~/generated'
import { useI18n } from '~/plugins/i18n'

/**
 * Where an import stands. The server does not serve while an import runs, so a running one here
 * was interrupted; a complete one says whether it matched its source.
 */
const props = defineProps<{
  state: ImportState
  reconciled?: boolean
}>()

const { t } = useI18n()
const badge = computed(() => {
  if (props.state === 'running') return { variant: 'warning', label: t('imports.states.unfinished') } as const
  if (props.reconciled === false) return { variant: 'destructive', label: t('imports.states.differs') } as const
  return { variant: 'success', label: t('imports.states.matches') } as const
})
</script>

<template>
  <Badge
    :variant="badge.variant"
    size="sm"
  >
    {{ badge.label }}
  </Badge>
</template>
