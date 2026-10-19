<script setup lang="ts">
// Adapted from holocron (84fc54ec): apps/web/src/components/account/SecretReveal.vue.
import CopyButton from '~/components/CopyButton.vue'
import { Alert } from '~/components/ui/alert'
import { Input } from '~/components/ui/input'
import { Label } from '~/components/ui/label'
import { useI18n } from '~/plugins/i18n'

/**
 * A secret the server discloses once: an API key or an invitation link. A replayed answer
 * withholds it (`secret` is `null`), and this says so instead of showing an empty field, since
 * the secret cannot be shown again.
 */
defineProps<{
  id: string
  label: string
  secret: string | null
}>()

const { t } = useI18n()

const select = (event: FocusEvent) => {
  if (event.target instanceof HTMLInputElement) event.target.select()
}
</script>

<template>
  <div class="grid gap-3">
    <template v-if="secret">
      <Alert color="warning">
        <p class="text-sm">{{ t('secrets.once') }}</p>
      </Alert>
      <div class="grid gap-1.5">
        <Label :for="id">{{ label }}</Label>
        <div class="flex gap-2 max-sm:flex-col">
          <Input
            :id="id"
            :model-value="secret"
            readonly
            class="font-mono"
            @focus="select"
          />
          <CopyButton :text="secret" />
        </div>
      </div>
    </template>
    <Alert
      v-else
      color="info"
    >
      <p class="text-sm">{{ t('secrets.withheld') }}</p>
    </Alert>
  </div>
</template>
