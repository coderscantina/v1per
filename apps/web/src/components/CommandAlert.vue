<script setup lang="ts">
// Adapted from holocron (84fc54ec): apps/web/src/pages/settings/CommandAlert.vue.
import { ApiError } from '~/api'
import { Alert } from '~/components/ui/alert'
import { Button } from '~/components/ui/button'
import { useErrorMessage } from '~/composables/useErrorMessage'
import { useI18n } from '~/plugins/i18n'

/**
 * A failed command. Field errors of the fields named in `shown` render at those fields, so they
 * are left out here; any other field error is said here, so a refusal is never silent.
 */
const props = defineProps<{ error: unknown; shown?: string[] }>()
const emit = defineEmits<{ reload: [] }>()

const { t } = useI18n()
const { message, fieldMessage } = useErrorMessage()

const code = computed(() => (props.error instanceof ApiError ? props.error.code : null))
const unshown = computed(() => {
  const error = props.error
  if (!(error instanceof ApiError) || error.code !== 'invalid_arguments') return []
  return Object.keys(error.body.errors ?? {})
    .filter((field) => !props.shown?.includes(field))
    .map((field) => fieldMessage(error, field) ?? '')
})
</script>

<template>
  <Alert
    v-if="code === 'stale_revision'"
    color="warning"
  >
    <p class="text-sm">{{ t('errors.stale_revision') }}</p>
    <Button
      type="button"
      variant="outline"
      size="sm"
      class="mt-2"
      @click="emit('reload')"
    >
      {{ t('actions.reload') }}
    </Button>
  </Alert>
  <Alert
    v-else-if="error && code !== 'invalid_arguments'"
    color="destructive"
  >
    <p class="text-sm">{{ message(error) }}</p>
  </Alert>
  <Alert
    v-else-if="unshown.length > 0"
    color="destructive"
  >
    <p class="text-sm">{{ message(error) }}</p>
    <ul class="mt-1 list-disc pl-5 text-sm">
      <li
        v-for="(text, index) in unshown"
        :key="index"
      >
        {{ text }}
      </li>
    </ul>
  </Alert>
</template>
