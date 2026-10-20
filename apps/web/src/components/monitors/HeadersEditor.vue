<script setup lang="ts">
import Icon from '~/components/Icon.vue'
import { Button } from '~/components/ui/button'
import { Checkbox } from '~/components/ui/checkbox'
import { Input } from '~/components/ui/input'
import { Label } from '~/components/ui/label'
import { MONITOR_HEADERS_MAX } from '~/generated/limits'
import { newSecret, type HeaderInput } from '~/lib/monitor-form'
import { useI18n } from '~/plugins/i18n'

import SecretInputField from './SecretInputField.vue'

/** Request headers as rows of name and value; a sealed row takes its value as a secret. */
const props = defineProps<{ id: string; error?: string | null }>()

const headers = defineModel<HeaderInput[]>({ required: true })
const { t } = useI18n()

const update = (at: number, patch: Partial<HeaderInput>) => {
  headers.value = headers.value.map((header, index) => (index === at ? { ...header, ...patch } : header))
}
const add = () => {
  headers.value = [...headers.value, { name: '', value: '', secret: null }]
}
const remove = (at: number) => {
  headers.value = headers.value.filter((_, index) => index !== at)
}
const seal = (at: number, sealed: boolean) => update(at, { secret: sealed ? newSecret() : null, value: '' })
</script>

<template>
  <fieldset
    class="grid gap-3"
    :aria-describedby="`${props.id}-description${error ? ` ${props.id}-error` : ''}`"
  >
    <legend class="mb-1.5 text-sm leading-none font-medium">{{ t('monitors.editor.headers') }}</legend>
    <div
      v-for="(header, at) in headers"
      :key="at"
      class="grid gap-2 rounded-control border border-hairline p-3 sm:grid-cols-[minmax(0,1fr)_minmax(0,1.4fr)_auto]"
    >
      <div class="grid gap-1.5">
        <Label :for="`${id}-${at}-name`">{{ t('monitors.editor.headerName') }}</Label>
        <Input
          :id="`${id}-${at}-name`"
          :model-value="header.name"
          autocomplete="off"
          @update:model-value="(value) => update(at, { name: String(value) })"
        />
      </div>
      <SecretInputField
        v-if="header.secret"
        :id="`${id}-${at}-secret`"
        :model-value="header.secret"
        :label="t('monitors.editor.headerValue')"
        @update:model-value="(secret) => update(at, { secret })"
      />
      <div
        v-else
        class="grid gap-1.5"
      >
        <Label :for="`${id}-${at}-value`">{{ t('monitors.editor.headerValue') }}</Label>
        <Input
          :id="`${id}-${at}-value`"
          :model-value="header.value"
          autocomplete="off"
          @update:model-value="(value) => update(at, { value: String(value) })"
        />
      </div>
      <div class="flex items-center gap-3 sm:mt-6 sm:self-start">
        <div class="flex items-center gap-2">
          <Checkbox
            :id="`${id}-${at}-sealed`"
            :model-value="header.secret !== null"
            @update:model-value="(value) => seal(at, value === true)"
          />
          <Label :for="`${id}-${at}-sealed`">{{ t('monitors.editor.headerSealed') }}</Label>
        </div>
        <Button
          type="button"
          variant="ghost"
          size="icon"
          :aria-label="t('monitors.editor.removeHeader', { name: header.name || String(at + 1) })"
          @click="remove(at)"
        >
          <Icon name="lucide:trash-2" />
        </Button>
      </div>
    </div>
    <p
      :id="`${id}-description`"
      class="text-sm text-muted"
    >
      {{ t('monitors.editor.headersHint') }}
    </p>
    <p
      v-if="error"
      :id="`${id}-error`"
      class="text-sm font-medium text-destructive"
    >
      {{ error }}
    </p>
    <Button
      type="button"
      variant="fill"
      size="sm"
      class="justify-self-start"
      :disabled="headers.length >= MONITOR_HEADERS_MAX"
      @click="add"
    >
      <Icon name="lucide:plus" />
      {{ t('monitors.editor.addHeader') }}
    </Button>
  </fieldset>
</template>
