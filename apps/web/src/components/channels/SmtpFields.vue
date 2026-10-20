<script setup lang="ts">
import FormField from '~/components/FormField.vue'
import SecretInputField from '~/components/monitors/SecretInputField.vue'
import SelectField from '~/components/SelectField.vue'
import SwitchField from '~/components/SwitchField.vue'
import TextareaField from '~/components/TextareaField.vue'
import { smtpSecurities, type SmtpInput, type SmtpSecurity } from '~/lib/channel-form'
import { useI18n, type MessageKey } from '~/plugins/i18n'

import TemplateHelp from './TemplateHelp.vue'

/**
 * Kuma's email options: the server (the instance's unless the channel has its own), sender and
 * recipients, subject and body templates, and the rarely used headers and DKIM signing folded
 * away.
 */
defineProps<{ errorFor: (key: string) => string | null }>()
const model = defineModel<SmtpInput>({ required: true })

const { t } = useI18n()
const securityLabels: Record<SmtpSecurity, MessageKey> = {
  starttls: 'channels.smtp.securities.starttls',
  tls: 'channels.smtp.securities.tls',
}
const securityOptions = computed(() => smtpSecurities.map((value) => ({ value, label: t(securityLabels[value]) })))
</script>

<template>
  <div class="grid gap-4">
    <SwitchField
      id="channel-smtp-own"
      v-model="model.ownServer"
      :label="t('channels.smtp.ownServer')"
      :description="model.ownServer ? undefined : t('channels.smtp.instanceServerHint')"
    />
    <template v-if="model.ownServer">
      <div class="grid gap-4 sm:grid-cols-[1fr_8rem]">
        <FormField
          id="channel-smtp-host"
          v-model="model.host"
          autocomplete="off"
          :label="t('channels.smtp.host')"
          :error="errorFor('settings.host')"
          required
        />
        <FormField
          id="channel-smtp-port"
          v-model="model.port"
          inputmode="numeric"
          :label="t('channels.smtp.port')"
          :error="errorFor('settings.port')"
          required
        />
      </div>
      <SelectField
        id="channel-smtp-security"
        v-model="model.security"
        :label="t('channels.smtp.security')"
        :options="securityOptions"
      />
      <SwitchField
        v-if="model.security === 'starttls'"
        id="channel-smtp-ignore-starttls"
        v-model="model.ignoreStarttls"
        :label="t('channels.smtp.ignoreStarttls')"
      />
      <SwitchField
        id="channel-smtp-ignore-tls"
        v-model="model.ignoreTlsError"
        :label="t('channels.smtp.ignoreTlsError')"
      />
      <div class="grid gap-4 sm:grid-cols-2">
        <FormField
          id="channel-smtp-username"
          v-model="model.username"
          autocomplete="off"
          :label="t('channels.smtp.username')"
          :error="errorFor('settings.username')"
        />
        <SecretInputField
          id="channel-smtp-password"
          v-model="model.password"
          :label="t('channels.smtp.password')"
          :error="errorFor('settings.password')"
        />
      </div>
    </template>
    <FormField
      id="channel-smtp-from"
      v-model="model.from"
      autocomplete="off"
      :label="t('channels.smtp.from')"
      :description="t('channels.smtp.fromHint')"
      :error="errorFor('settings.from')"
    />
    <FormField
      id="channel-smtp-to"
      v-model="model.to"
      autocomplete="off"
      :label="t('channels.smtp.to')"
      :description="t('channels.smtp.recipientsHint')"
      :error="errorFor('settings.to')"
    />
    <div class="grid gap-4 sm:grid-cols-2">
      <FormField
        id="channel-smtp-cc"
        v-model="model.cc"
        autocomplete="off"
        :label="t('channels.smtp.cc')"
        :error="errorFor('settings.cc')"
      />
      <FormField
        id="channel-smtp-bcc"
        v-model="model.bcc"
        autocomplete="off"
        :label="t('channels.smtp.bcc')"
        :error="errorFor('settings.bcc')"
      />
    </div>
    <FormField
      id="channel-smtp-subject"
      v-model="model.customSubject"
      autocomplete="off"
      :label="t('channels.smtp.subject')"
      :error="errorFor('settings.custom_subject')"
    />
    <TextareaField
      id="channel-smtp-body"
      v-model="model.customBody"
      code
      :rows="5"
      :label="t('channels.smtp.body')"
      :error="errorFor('settings.custom_body')"
      described-by="channel-smtp-templates"
    />
    <SwitchField
      id="channel-smtp-html"
      v-model="model.htmlBody"
      :label="t('channels.smtp.htmlBody')"
    />
    <TemplateHelp id="channel-smtp-templates" />
    <details class="group grid gap-4">
      <summary
        class="cursor-pointer rounded-sm text-sm font-medium text-primary focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring"
      >
        {{ t('channels.smtp.advanced') }}
      </summary>
      <div class="mt-4 grid gap-4">
        <SecretInputField
          id="channel-smtp-headers"
          v-model="model.additionalHeaders"
          :label="t('channels.smtp.headers')"
          :error="errorFor('settings.additional_headers')"
        />
        <div class="grid gap-4 sm:grid-cols-2">
          <FormField
            id="channel-smtp-dkim-domain"
            v-model="model.dkimDomain"
            autocomplete="off"
            :label="t('channels.smtp.dkimDomain')"
            :error="errorFor('settings.dkim_domain')"
          />
          <FormField
            id="channel-smtp-dkim-selector"
            v-model="model.dkimKeySelector"
            autocomplete="off"
            :label="t('channels.smtp.dkimKeySelector')"
            :error="errorFor('settings.dkim_key_selector')"
          />
        </div>
        <SecretInputField
          id="channel-smtp-dkim-key"
          v-model="model.dkimPrivateKey"
          :label="t('channels.smtp.dkimPrivateKey')"
          :error="errorFor('settings.dkim_private_key')"
        />
        <div class="grid gap-4 sm:grid-cols-3">
          <FormField
            id="channel-smtp-dkim-hash"
            v-model="model.dkimHashAlgo"
            autocomplete="off"
            placeholder="sha256"
            :label="t('channels.smtp.dkimHashAlgo')"
          />
          <FormField
            id="channel-smtp-dkim-headers"
            v-model="model.dkimHeaderFieldNames"
            autocomplete="off"
            :label="t('channels.smtp.dkimHeaderFieldNames')"
          />
          <FormField
            id="channel-smtp-dkim-skip"
            v-model="model.dkimSkipFields"
            autocomplete="off"
            :label="t('channels.smtp.dkimSkipFields')"
          />
        </div>
      </div>
    </details>
  </div>
</template>
