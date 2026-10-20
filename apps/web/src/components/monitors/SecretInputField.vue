<script setup lang="ts">
import FieldMessages from '~/components/FieldMessages.vue'
import { Badge } from '~/components/ui/badge'
import { Button } from '~/components/ui/button'
import { Input } from '~/components/ui/input'
import { Label } from '~/components/ui/label'
import type { SecretInput } from '~/lib/monitor-form'
import { useI18n } from '~/plugins/i18n'

/**
 * A sealed credential. A stored one shows only that it is set, with Replace and Clear; the value
 * typed for a new or replaced one is sent once and never shown again.
 */
const props = defineProps<{ id: string; label: string; error?: string | null }>()

const model = defineModel<SecretInput>({ required: true })
const { t } = useI18n()

const set = (mode: SecretInput['mode']) => {
  model.value = { ...model.value, mode, value: '' }
}
const editing = computed(() => !model.value.set || model.value.mode === 'replace')
const describedBy = computed(() => [editing.value ? `${props.id}-description` : null, props.error ? `${props.id}-error` : null].filter(Boolean).join(' '))
</script>

<template>
  <div class="grid content-start gap-1.5">
    <Label :for="editing ? id : undefined">{{ label }}</Label>
    <div
      v-if="editing"
      class="flex gap-2"
    >
      <Input
        :id="id"
        :model-value="model.value"
        type="password"
        autocomplete="off"
        :aria-invalid="error ? 'true' : undefined"
        :aria-describedby="describedBy"
        @update:model-value="(value) => (model = { ...model, value: String(value) })"
      />
      <Button
        v-if="model.set"
        type="button"
        variant="fill"
        @click="set('keep')"
      >
        {{ t('monitors.editor.secret.keep') }}
      </Button>
    </div>
    <div
      v-else
      class="flex min-h-9 flex-wrap items-center gap-2"
    >
      <Badge
        v-if="model.mode === 'keep'"
        variant="success"
        size="sm"
      >
        {{ t('monitors.editor.secret.set') }}
      </Badge>
      <span
        v-else
        class="text-sm text-muted"
      >{{ t('monitors.editor.secret.cleared') }}</span>
      <Button
        type="button"
        variant="fill"
        size="sm"
        :aria-label="`${t('monitors.editor.secret.replace')}: ${label}`"
        @click="set('replace')"
      >
        {{ t('monitors.editor.secret.replace') }}
      </Button>
      <Button
        v-if="model.mode === 'keep'"
        type="button"
        variant="ghost"
        size="sm"
        :aria-label="`${t('monitors.editor.secret.clear')}: ${label}`"
        @click="set('clear')"
      >
        {{ t('monitors.editor.secret.clear') }}
      </Button>
      <Button
        v-else
        type="button"
        variant="ghost"
        size="sm"
        @click="set('keep')"
      >
        {{ t('monitors.editor.secret.keep') }}
      </Button>
    </div>
    <FieldMessages
      :id="id"
      :description="editing ? t('monitors.editor.secret.sealed') : undefined"
      :error="error"
    />
  </div>
</template>
