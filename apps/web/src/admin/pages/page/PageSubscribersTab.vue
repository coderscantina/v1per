<script setup lang="ts">
import { useMutation } from '@tanstack/vue-query'

import { pagesResource } from '~/api/resources/pages'
import CommandAlert from '~/components/CommandAlert.vue'
import FormField from '~/components/FormField.vue'
import Icon from '~/components/Icon.vue'
import { Alert } from '~/components/ui/alert'
import { Button } from '~/components/ui/button'
import { Panel } from '~/components/ui/panel'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '~/components/ui/table'
import { useConfirm } from '~/composables/useConfirm'
import { useDateFormat } from '~/composables/useDateFormat'
import { useErrorMessage } from '~/composables/useErrorMessage'
import { usePageEditor, usePageSubscribers } from '~/composables/usePages'
import type { PageComponentId, PageSubscriber, PageWebhook } from '~/generated'
import { PAGE_SUBSCRIBERS_MAX, PAGE_WEBHOOKS_MAX } from '~/generated/limits'
import { useI18n } from '~/plugins/i18n'

/**
 * Who follows a page: the addresses that confirmed a subscription, and the webhooks its
 * managers added. Visitors subscribe on the page itself; only managers add URLs.
 */
const { t } = useI18n()
const { fieldMessage, message } = useErrorMessage()
const { confirm } = useConfirm()
const { date } = useDateFormat()
const editor = usePageEditor()
const detail = computed(() => editor.data.value ?? null)
const page = computed(() => detail.value?.page ?? null)
const live = usePageSubscribers(() => (editor.manage.value ? (page.value?.id ?? null) : null))
const subscribers = computed(() => live.data.value?.subscribers ?? [])
const webhooks = computed(() => live.data.value?.webhooks ?? [])

const names = computed(
  () => new Map(detail.value?.groups.flatMap((group) => group.components.map((component) => [component.id, component.name] as const)))
)
/** What a subscriber or webhook follows, in words. */
const follows = (components: PageComponentId[]) =>
  components.length === 0 ? t('pages.subscribers.everything') : components.map((id) => names.value.get(id) ?? t('pages.subscribers.removedComponent')).join(', ')

const removeSubscriber = useMutation({ mutationFn: (subscriber: PageSubscriber) => pagesResource.removeSubscriber(subscriber.id) })
const askRemoveSubscriber = async (subscriber: PageSubscriber) => {
  const ok = await confirm({
    title: t('pages.subscribers.removeTitle'),
    message: t('pages.subscribers.removeConfirm', { email: subscriber.email }),
    confirmLabel: t('pages.subscribers.remove'),
    variant: 'destructive',
  })
  if (ok) removeSubscriber.mutate(subscriber)
}

const url = ref('')
const addWebhook = useMutation({
  mutationFn: () => pagesResource.addWebhook({ page: page.value?.id ?? '', url: url.value.trim() }),
  onSuccess: () => (url.value = ''),
})
const removeWebhook = useMutation({ mutationFn: (webhook: PageWebhook) => pagesResource.removeWebhook(webhook.id) })
const askRemoveWebhook = async (webhook: PageWebhook) => {
  const ok = await confirm({
    title: t('pages.subscribers.removeWebhookTitle'),
    message: t('pages.subscribers.removeWebhookConfirm', { host: webhook.host }),
    confirmLabel: t('pages.subscribers.remove'),
    variant: 'destructive',
  })
  if (ok) removeWebhook.mutate(webhook)
}
</script>

<template>
  <Alert
    v-if="!editor.manage.value"
    icon="lucide:info"
  >
    <p class="text-sm">{{ t('pages.readOnly') }}</p>
  </Alert>
  <div
    v-else-if="page && detail"
    class="grid gap-6"
  >
    <p class="text-sm text-muted">{{ t('pages.subscribers.intro') }}</p>
    <Alert
      v-if="!page.subscriptions"
      icon="lucide:info"
    >
      <p class="text-sm">{{ t('pages.subscribers.off') }}</p>
    </Alert>
    <Alert
      v-else-if="!detail.smtp_configured"
      color="warning"
    >
      <p class="text-sm">{{ t('pages.subscribers.noSmtp') }}</p>
    </Alert>
    <Alert
      v-if="live.error.value"
      color="destructive"
    >
      <p class="text-sm">{{ message(live.error.value) }}</p>
    </Alert>

    <Panel
      id="page-subscribers"
      :title="t('pages.subscribers.emails')"
      :meta="`${subscribers.length} / ${PAGE_SUBSCRIBERS_MAX}`"
    >
      <div class="grid gap-4">
        <Alert
          v-if="removeSubscriber.error.value"
          color="destructive"
        >
          <p class="text-sm">{{ message(removeSubscriber.error.value) }}</p>
        </Alert>
        <Table
          v-if="subscribers.length > 0"
          :label="t('pages.subscribers.emails')"
        >
          <TableHeader>
            <TableRow>
              <TableHead>{{ t('pages.subscribers.email') }}</TableHead>
              <TableHead class="max-sm:hidden">{{ t('pages.subscribers.follows') }}</TableHead>
              <TableHead class="max-sm:hidden">{{ t('pages.subscribers.since') }}</TableHead>
              <TableHead><span class="sr-only">{{ t('actions.label') }}</span></TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            <TableRow
              v-for="subscriber in subscribers"
              :key="subscriber.id"
              :data-subscriber="subscriber.email"
            >
              <TableCell
                kind="subject"
                class="max-w-0 min-w-40"
              >
                <span class="block truncate">{{ subscriber.email }}</span>
              </TableCell>
              <TableCell
                kind="secondary"
                class="max-w-0 min-w-32 max-sm:hidden"
              >
                <span class="block truncate">{{ follows(subscriber.components) }}</span>
              </TableCell>
              <TableCell
                kind="secondary"
                class="whitespace-nowrap max-sm:hidden"
              >
                {{ date(subscriber.confirmed_at) }}
              </TableCell>
              <TableCell class="text-right">
                <Button
                  type="button"
                  variant="ghost"
                  size="icon"
                  :disabled="removeSubscriber.isPending.value"
                  :aria-label="t('pages.subscribers.removeNamed', { name: subscriber.email })"
                  @click="askRemoveSubscriber(subscriber)"
                >
                  <Icon name="lucide:trash-2" />
                </Button>
              </TableCell>
            </TableRow>
          </TableBody>
        </Table>
        <p
          v-else
          class="py-4 text-center text-sm text-muted"
        >
          {{ t('pages.subscribers.none') }}
        </p>
      </div>
    </Panel>

    <Panel
      id="page-webhooks"
      :title="t('pages.subscribers.webhooks')"
      :meta="`${webhooks.length} / ${PAGE_WEBHOOKS_MAX}`"
    >
      <div class="grid gap-4">
        <p class="text-sm text-muted">{{ t('pages.subscribers.webhooksIntro') }}</p>
        <form
          class="grid gap-3 sm:grid-cols-[minmax(0,1fr)_auto] sm:items-start"
          novalidate
          @submit.prevent="addWebhook.mutate()"
        >
          <FormField
            id="page-webhook-url"
            v-model="url"
            type="url"
            autocomplete="off"
            placeholder="https://"
            :label="t('pages.subscribers.webhookUrl')"
            :error="fieldMessage(addWebhook.error.value, 'url')"
            required
          />
          <Button
            type="submit"
            variant="fill"
            class="sm:mt-6"
            :loading="addWebhook.isPending.value"
            :disabled="url.trim() === ''"
          >
            <Icon name="lucide:plus" />
            {{ t('pages.subscribers.addWebhook') }}
          </Button>
          <CommandAlert
            class="sm:col-span-2"
            :error="addWebhook.error.value"
            :shown="['url']"
          />
        </form>
        <Alert
          v-if="removeWebhook.error.value"
          color="destructive"
        >
          <p class="text-sm">{{ message(removeWebhook.error.value) }}</p>
        </Alert>
        <Table
          v-if="webhooks.length > 0"
          :label="t('pages.subscribers.webhooks')"
        >
          <TableHeader>
            <TableRow>
              <TableHead>{{ t('pages.subscribers.host') }}</TableHead>
              <TableHead class="max-sm:hidden">{{ t('pages.subscribers.follows') }}</TableHead>
              <TableHead class="max-sm:hidden">{{ t('pages.subscribers.added') }}</TableHead>
              <TableHead><span class="sr-only">{{ t('actions.label') }}</span></TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            <TableRow
              v-for="webhook in webhooks"
              :key="webhook.id"
              :data-webhook="webhook.host"
            >
              <TableCell
                kind="subject"
                class="max-w-0 min-w-40"
              >
                <span class="block truncate">{{ webhook.host }}</span>
              </TableCell>
              <TableCell
                kind="secondary"
                class="max-w-0 min-w-32 max-sm:hidden"
              >
                <span class="block truncate">{{ follows(webhook.components) }}</span>
              </TableCell>
              <TableCell
                kind="secondary"
                class="whitespace-nowrap max-sm:hidden"
              >
                {{ date(webhook.created_at) }}
              </TableCell>
              <TableCell class="text-right">
                <Button
                  type="button"
                  variant="ghost"
                  size="icon"
                  :disabled="removeWebhook.isPending.value"
                  :aria-label="t('pages.subscribers.removeNamed', { name: webhook.host })"
                  @click="askRemoveWebhook(webhook)"
                >
                  <Icon name="lucide:trash-2" />
                </Button>
              </TableCell>
            </TableRow>
          </TableBody>
        </Table>
      </div>
    </Panel>
  </div>
</template>
