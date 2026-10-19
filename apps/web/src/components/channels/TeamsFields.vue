<script setup lang="ts">
import SecretInputField from '~/components/monitors/SecretInputField.vue'
import SwitchField from '~/components/SwitchField.vue'
import type { TeamsInput } from '~/lib/channel-form'
import { useI18n } from '~/plugins/i18n'

/** Kuma's Microsoft Teams options. */
defineProps<{ errorFor: (key: string) => string | null }>()
const model = defineModel<TeamsInput>({ required: true })

const { t } = useI18n()
</script>

<template>
  <div class="grid gap-4">
    <SecretInputField
      id="channel-teams-webhook"
      v-model="model.webhookUrl"
      :label="t('channels.teams.webhookUrl')"
      :error="errorFor('settings.webhook_url')"
    />
    <SwitchField
      id="channel-teams-tags"
      v-model="model.enableTags"
      :label="t('channels.teams.enableTags')"
    />
  </div>
</template>
