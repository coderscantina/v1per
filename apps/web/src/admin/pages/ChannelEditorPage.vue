<script setup lang="ts">
import { useMutation } from '@tanstack/vue-query'

import { channelsResource } from '~/api/resources/channels'
import SettingsPage from '~/components/account/SettingsPage.vue'
import SettingsSection from '~/components/account/SettingsSection.vue'
import DiscordFields from '~/components/channels/DiscordFields.vue'
import NtfyFields from '~/components/channels/NtfyFields.vue'
import PushoverFields from '~/components/channels/PushoverFields.vue'
import SlackFields from '~/components/channels/SlackFields.vue'
import SmtpFields from '~/components/channels/SmtpFields.vue'
import TeamsFields from '~/components/channels/TeamsFields.vue'
import TelegramFields from '~/components/channels/TelegramFields.vue'
import TestSend from '~/components/channels/TestSend.vue'
import WebhookFields from '~/components/channels/WebhookFields.vue'
import CommandAlert from '~/components/CommandAlert.vue'
import FormField from '~/components/FormField.vue'
import PageHeader from '~/components/PageHeader.vue'
import SelectField from '~/components/SelectField.vue'
import SwitchField from '~/components/SwitchField.vue'
import { Alert } from '~/components/ui/alert'
import { Button } from '~/components/ui/button'
import { Skeleton } from '~/components/ui/skeleton'
import { useChannel } from '~/composables/useChannels'
import { useErrorMessage } from '~/composables/useErrorMessage'
import { useCurrentNode, useNodeDetail } from '~/composables/useNodes'
import { usePageMeta } from '~/composables/usePageMeta'
import type { ChannelResult, ChannelsTestArgs } from '~/generated'
import { NAME_MAX_CHARS } from '~/generated/limits'
import { can } from '~/lib/access'
import {
  coreProviders,
  emptyChannelForm,
  formFromChannel,
  toChannelInput,
  toChannelSettings,
  validateChannel,
  type ChannelForm,
} from '~/lib/channel-form'
import { providerLabels } from '~/lib/channels'
import { useI18n } from '~/plugins/i18n'

/**
 * Adds a notification channel to the node, or edits one, with Kuma's options per provider.
 * Switching the provider keeps what was typed for the others. A test sends the form as it
 * stands, stored secrets included. Later changes by others surface as a stale revision on save.
 */
const { t } = useI18n()
const { fieldMessage, fieldProblem } = useErrorMessage()
const route = useRoute()
const router = useRouter()
const { routeId } = useCurrentNode()
const node = useNodeDetail(routeId)
const manage = computed(() => can(node.data.value?.node, 'channels.manage'))

const channelId = computed(() => (typeof route.params.channelId === 'string' ? route.params.channelId : null))
const editing = computed(() => channelId.value !== null)
const live = useChannel(() => (manage.value ? channelId.value : null))
const stored = computed(() => live.data.value?.channel ?? null)
usePageMeta(() => (editing.value ? t('channels.editor.editTitle') : t('channels.editor.createTitle')))

const form = ref<ChannelForm>(emptyChannelForm())
/** The revision the form was read at; `null` until it is read. */
const basedOn = ref<number | null>(null)
const unsupported = ref(false)

const load = () => {
  const channel = stored.value
  if (!channel) return
  const next = formFromChannel(channel)
  unsupported.value = next === null
  if (next) form.value = next
  basedOn.value = channel.revision
}
watch(
  stored,
  () => {
    if (basedOn.value === null) load()
  },
  { immediate: true }
)

const submitted = ref(false)
const clientErrors = computed(() => (submitted.value ? validateChannel(form.value) : {}))

const save = useMutation({
  mutationFn: (current: ChannelForm): Promise<ChannelResult> => {
    const input = toChannelInput(current)
    const channel = channelId.value
    if (channel) return channelsResource.update({ ...input, channel, expected_revision: basedOn.value ?? undefined })
    return channelsResource.create({ ...input, node: routeId.value ?? '' })
  },
})

const errorFor = (key: string): string | null => {
  const local = clientErrors.value[key]
  if (local) return fieldProblem(local)
  return fieldMessage(save.error.value, key)
}

const focusInvalid = () => void nextTick(() => document.querySelector<HTMLElement>('[aria-invalid="true"]')?.focus())

const submit = () => {
  submitted.value = true
  if (Object.keys(validateChannel(form.value)).length > 0) {
    focusInvalid()
    return
  }
  save.mutate(form.value, {
    onSuccess: ({ channel }) => void router.push({ name: 'node-channel', params: { nodeId: channel.node, channelId: channel.id } }),
  })
}

/** The test needs what sending needs; the name may still be empty. */
const testArgs = (): ChannelsTestArgs | null => {
  submitted.value = true
  const errors = Object.keys(validateChannel(form.value)).filter((key) => key !== 'name')
  if (errors.length > 0) {
    focusInvalid()
    return null
  }
  return {
    node: routeId.value ?? '',
    channel: channelId.value ?? undefined,
    name: form.value.name.trim() || t('channels.editor.untitled'),
    settings: toChannelSettings(form.value),
  }
}

const reload = async () => {
  await live.reload()
  basedOn.value = null
  load()
  save.reset()
}

const providerOptions = computed(() => coreProviders.map((value) => ({ value, label: t(providerLabels[value]) })))

/** Fields that show their own server error; any other is said in the alert. */
const shownFields = [
  'name',
  'settings.access_token',
  'settings.additional_headers',
  'settings.app_token',
  'settings.bcc',
  'settings.bot_token',
  'settings.cc',
  'settings.channel',
  'settings.chat_id',
  'settings.custom_body',
  'settings.custom_message',
  'settings.custom_subject',
  'settings.custom_title',
  'settings.dkim_domain',
  'settings.dkim_key_selector',
  'settings.dkim_private_key',
  'settings.from',
  'settings.host',
  'settings.message_template',
  'settings.password',
  'settings.port',
  'settings.post_name',
  'settings.prefix_message',
  'settings.priority',
  'settings.priority_down',
  'settings.server_url',
  'settings.template',
  'settings.thread_id',
  'settings.to',
  'settings.topic',
  'settings.ttl',
  'settings.url',
  'settings.user_key',
  'settings.username',
  'settings.webhook_url',
]

const ready = computed(() => node.data.value && (!editing.value || basedOn.value !== null || !manage.value))
const backTo = computed(() =>
  channelId.value
    ? { name: 'node-channel', params: { nodeId: routeId.value, channelId: channelId.value } }
    : { name: 'node-channels', params: { nodeId: routeId.value } }
)
</script>

<template>
  <div>
    <PageHeader
      :title="editing ? t('channels.editor.editTitle') : t('channels.editor.createTitle')"
      :description="stored?.name ?? node.data.value?.node.name"
    />

    <Alert
      v-if="(node.error.value && !node.data.value) || (live.error.value && !stored)"
      color="destructive"
      class="max-w-4xl"
    >
      <p class="text-sm">{{ t('channels.unavailable') }}</p>
    </Alert>
    <div
      v-else-if="!ready"
      class="grid max-w-4xl gap-6"
    >
      <Skeleton class="h-32 w-full rounded-panel" />
      <Skeleton class="h-56 w-full rounded-panel" />
    </div>
    <Alert
      v-else-if="!manage"
      icon="lucide:info"
      class="max-w-4xl"
    >
      <p class="text-sm">{{ t('channels.readOnly') }}</p>
    </Alert>
    <Alert
      v-else-if="unsupported"
      color="warning"
      class="max-w-4xl"
    >
      <p class="text-sm">{{ t('channels.editor.unsupported', { reason: stored?.inactive_reason ?? '' }) }}</p>
    </Alert>

    <form
      v-else
      class="max-w-4xl"
      novalidate
      @submit.prevent="submit"
    >
      <SettingsPage>
        <SettingsSection
          id="channel-basics"
          :title="t('channels.editor.basics')"
          :description="t('channels.editor.basicsHint')"
        >
          <SelectField
            id="channel-provider"
            v-model="form.provider"
            :label="t('channels.editor.provider')"
            :options="providerOptions"
            :description="t('channels.editor.providerHint')"
          />
          <FormField
            id="channel-name"
            v-model="form.name"
            autocomplete="off"
            :label="t('channels.editor.name')"
            :description="t('nodes.nameHint', { max: NAME_MAX_CHARS })"
            :error="errorFor('name')"
            required
          />
          <SwitchField
            id="channel-default"
            v-model="form.isDefault"
            :label="t('channels.editor.isDefault')"
            :description="t('channels.editor.isDefaultHint')"
          />
          <SwitchField
            id="channel-apply-existing"
            v-model="form.applyExisting"
            :label="t('channels.editor.applyExisting')"
            :description="t('channels.editor.applyExistingHint')"
          />
          <SwitchField
            id="channel-active"
            v-model="form.active"
            :label="t('channels.editor.active')"
            :description="t('channels.editor.activeHint')"
          />
        </SettingsSection>

        <SettingsSection
          id="channel-settings"
          :title="t(providerLabels[form.provider])"
        >
          <SmtpFields
            v-if="form.provider === 'smtp'"
            v-model="form.smtp"
            :error-for="errorFor"
          />
          <DiscordFields
            v-else-if="form.provider === 'discord'"
            v-model="form.discord"
            :error-for="errorFor"
          />
          <WebhookFields
            v-else-if="form.provider === 'webhook'"
            v-model="form.webhook"
            :error-for="errorFor"
          />
          <SlackFields
            v-else-if="form.provider === 'slack'"
            v-model="form.slack"
            :error-for="errorFor"
          />
          <TeamsFields
            v-else-if="form.provider === 'teams'"
            v-model="form.teams"
            :error-for="errorFor"
          />
          <TelegramFields
            v-else-if="form.provider === 'telegram'"
            v-model="form.telegram"
            :error-for="errorFor"
          />
          <NtfyFields
            v-else-if="form.provider === 'ntfy'"
            v-model="form.ntfy"
            :error-for="errorFor"
          />
          <PushoverFields
            v-else
            v-model="form.pushover"
            :error-for="errorFor"
          />
        </SettingsSection>

        <SettingsSection
          id="channel-test"
          :title="t('channels.test.title')"
          :description="t('channels.test.hint')"
        >
          <TestSend
            :node="routeId"
            :args="testArgs"
            :send="channelsResource.test"
          />
        </SettingsSection>
      </SettingsPage>

      <div class="mt-8 grid gap-4 border-t border-divider pt-6">
        <Alert
          v-if="submitted && Object.keys(clientErrors).length > 0"
          color="destructive"
        >
          <p class="text-sm">{{ t('channels.editor.invalid') }}</p>
        </Alert>
        <CommandAlert
          v-else-if="save.error.value"
          :error="save.error.value"
          :shown="shownFields"
          @reload="reload"
        />
        <div class="flex flex-wrap gap-2">
          <Button
            type="submit"
            variant="accent"
            :loading="save.isPending.value"
          >
            {{ editing ? t('channels.editor.saveEdit') : t('channels.editor.saveCreate') }}
          </Button>
          <Button
            as-child
            variant="fill"
          >
            <RouterLink :to="backTo">{{ t('actions.cancel') }}</RouterLink>
          </Button>
        </div>
      </div>
    </form>
  </div>
</template>
