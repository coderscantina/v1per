<script setup lang="ts">
import FormField from '~/components/FormField.vue'
import SecretInputField from '~/components/monitors/SecretInputField.vue'
import type { PushoverInput } from '~/lib/channel-form'
import { useI18n } from '~/plugins/i18n'

/** Kuma's Pushover options. */
defineProps<{ errorFor: (key: string) => string | null }>()
const model = defineModel<PushoverInput>({ required: true })

const { t } = useI18n()
</script>

<template>
  <div class="grid gap-4">
    <div class="grid gap-4 sm:grid-cols-2">
      <SecretInputField
        id="channel-pushover-user"
        v-model="model.userKey"
        :label="t('channels.pushover.userKey')"
        :error="errorFor('settings.user_key')"
      />
      <SecretInputField
        id="channel-pushover-app"
        v-model="model.appToken"
        :label="t('channels.pushover.appToken')"
        :error="errorFor('settings.app_token')"
      />
    </div>
    <div class="grid gap-4 sm:grid-cols-2">
      <FormField
        id="channel-pushover-device"
        v-model="model.device"
        autocomplete="off"
        :label="t('channels.pushover.device')"
      />
      <FormField
        id="channel-pushover-title"
        v-model="model.title"
        autocomplete="off"
        :label="t('channels.pushover.title')"
      />
      <FormField
        id="channel-pushover-sound"
        v-model="model.sound"
        autocomplete="off"
        :label="t('channels.pushover.sound')"
      />
      <FormField
        id="channel-pushover-sound-up"
        v-model="model.soundUp"
        autocomplete="off"
        :label="t('channels.pushover.soundUp')"
      />
      <FormField
        id="channel-pushover-priority"
        v-model="model.priority"
        inputmode="numeric"
        :label="t('channels.pushover.priority')"
        :placeholder="t('channels.pushover.priorityHint')"
        :error="errorFor('settings.priority')"
      />
      <FormField
        id="channel-pushover-ttl"
        v-model="model.ttl"
        inputmode="numeric"
        :label="t('channels.pushover.ttl')"
        :suffix="t('units.seconds')"
        :error="errorFor('settings.ttl')"
      />
    </div>
  </div>
</template>
