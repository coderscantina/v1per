<script setup lang="ts">
import { useMutation } from '@tanstack/vue-query'

import { channelsResource } from '~/api/resources/channels'
import DeliveryTable from '~/components/channels/DeliveryTable.vue'
import TestSend from '~/components/channels/TestSend.vue'
import Icon from '~/components/Icon.vue'
import PageHeader from '~/components/PageHeader.vue'
import { Alert } from '~/components/ui/alert'
import { Badge } from '~/components/ui/badge'
import { Button } from '~/components/ui/button'
import { EmptyState } from '~/components/ui/empty-state'
import { Panel } from '~/components/ui/panel'
import { Skeleton } from '~/components/ui/skeleton'
import { useChannel, useChannelDeliveries } from '~/composables/useChannels'
import { useConfirm } from '~/composables/useConfirm'
import { useDateFormat } from '~/composables/useDateFormat'
import { useErrorMessage } from '~/composables/useErrorMessage'
import { useCurrentNode } from '~/composables/useNodes'
import { usePageMeta } from '~/composables/usePageMeta'
import type { ChannelsTestArgs } from '~/generated'
import { providerIcon, providerName, providerOf } from '~/lib/channels'
import { useI18n } from '~/plugins/i18n'

/**
 * One channel: what it is, a test send with the stored settings, and its delivery log live,
 * newest first, with attempts, the last error and when a retry is due.
 */
const { t } = useI18n()
const { message } = useErrorMessage()
const { confirm } = useConfirm()
const { dateTime } = useDateFormat()
const route = useRoute()
const router = useRouter()
const { routeId } = useCurrentNode()

const channelId = computed(() => (typeof route.params.channelId === 'string' ? route.params.channelId : null))
const live = useChannel(channelId)
const channel = computed(() => live.data.value?.channel ?? null)
usePageMeta(() => channel.value?.name ?? t('channels.title'))

// A link from another node's page may name that node; the channel's own node scopes the page.
watch(channel, (current) => {
  if (current && current.node !== routeId.value) {
    void router.replace({ name: 'node-channel', params: { nodeId: current.node, channelId: current.id } })
  }
})

/** The log grows a page at a time up to the server's page limit. */
const PAGE = 25
const MAX = 100
const limit = ref(PAGE)
const deliveries = useChannelDeliveries(() => (channel.value ? channel.value.id : null), limit)
const rows = computed(() => deliveries.data.value?.deliveries ?? null)
const more = computed(() => Boolean(deliveries.data.value?.next_cursor))

const testArgs = (): ChannelsTestArgs | null =>
  channel.value ? { node: channel.value.node, channel: channel.value.id, name: channel.value.name, settings: channel.value.settings } : null

const remove = useMutation({ mutationFn: (id: string) => channelsResource.delete(id) })
const ask = async () => {
  const current = channel.value
  if (!current) return
  const ok = await confirm({
    title: t('channels.deleteTitle'),
    message:
      current.monitors > 0
        ? t('channels.deleteConfirmUsed', { name: current.name, count: current.monitors }, current.monitors)
        : t('channels.deleteConfirm', { name: current.name }),
    confirmLabel: t('channels.delete'),
    variant: 'destructive',
  })
  if (ok) {
    remove.mutate(current.id, {
      onSuccess: () => void router.replace({ name: 'node-channels', params: { nodeId: current.node } }),
    })
  }
}
</script>

<template>
  <div>
    <PageHeader
      :title="channel?.name ?? t('channels.title')"
      :description="channel ? providerName(providerOf(channel.settings), t) : undefined"
    >
      <template
        v-if="channel"
        #actions
      >
        <Button
          v-if="channel.settings.type !== 'unsupported'"
          as-child
          variant="fill"
        >
          <RouterLink :to="{ name: 'node-channel-edit', params: { nodeId: channel.node, channelId: channel.id } }">
            <Icon name="lucide:pencil" />
            {{ t('channels.edit') }}
          </RouterLink>
        </Button>
        <Button
          variant="destructive"
          :disabled="remove.isPending.value"
          @click="ask"
        >
          <Icon name="lucide:trash-2" />
          {{ t('channels.delete') }}
        </Button>
      </template>
    </PageHeader>

    <EmptyState
      v-if="live.error.value && !channel"
      icon="lucide:bell-off"
      tone="error"
      :title="t('channels.unavailable')"
      :description="message(live.error.value)"
      class="mx-auto max-w-lg"
    >
      <Button
        as-child
        variant="outline"
      >
        <RouterLink :to="{ name: 'node-channels', params: { nodeId: routeId } }">{{ t('channels.title') }}</RouterLink>
      </Button>
    </EmptyState>

    <div
      v-else-if="!channel"
      class="grid max-w-6xl gap-6"
    >
      <Skeleton class="h-36 w-full rounded-panel" />
      <Skeleton class="h-64 w-full rounded-panel" />
    </div>

    <div
      v-else
      class="grid max-w-6xl gap-6"
    >
      <Alert
        v-if="remove.error.value"
        color="destructive"
      >
        <p class="text-sm">{{ message(remove.error.value) }}</p>
      </Alert>
      <Alert
        v-if="!channel.active"
        color="warning"
      >
        <p class="text-sm">{{ channel.inactive_reason ?? t('channels.inactiveHint') }}</p>
      </Alert>

      <div class="grid gap-6 lg:grid-cols-2">
        <Panel
          id="channel-about"
          :title="t('channels.about')"
        >
          <dl class="grid gap-3 text-sm">
            <div class="grid gap-0.5 sm:grid-cols-[10rem_1fr] sm:gap-6">
              <dt class="text-muted">{{ t('channels.columns.provider') }}</dt>
              <dd class="inline-flex items-center gap-1.5 text-primary">
                <Icon
                  :name="providerIcon(providerOf(channel.settings))"
                  size="14"
                  aria-hidden="true"
                />
                {{ providerName(providerOf(channel.settings), t) }}
              </dd>
            </div>
            <div class="grid gap-0.5 sm:grid-cols-[10rem_1fr] sm:gap-6">
              <dt class="text-muted">{{ t('channels.columns.monitors') }}</dt>
              <dd class="text-primary tabular-nums">{{ channel.monitors }}</dd>
            </div>
            <div class="grid gap-0.5 sm:grid-cols-[10rem_1fr] sm:gap-6">
              <dt class="text-muted">{{ t('channels.columns.flags') }}</dt>
              <dd class="flex flex-wrap gap-1">
                <Badge
                  v-if="channel.is_default"
                  variant="secondary"
                  size="sm"
                >
                  {{ t('channels.default') }}
                </Badge>
                <Badge
                  :variant="channel.active ? 'success' : 'warning'"
                  size="sm"
                >
                  {{ channel.active ? t('channels.active') : t('channels.inactive') }}
                </Badge>
              </dd>
            </div>
            <div class="grid gap-0.5 sm:grid-cols-[10rem_1fr] sm:gap-6">
              <dt class="text-muted">{{ t('channels.updated') }}</dt>
              <dd class="text-primary">{{ dateTime(channel.updated_at) }}</dd>
            </div>
          </dl>
        </Panel>
        <Panel
          v-if="channel.settings.type !== 'unsupported'"
          id="channel-test"
          :title="t('channels.test.title')"
        >
          <p class="mb-3 text-sm text-muted">{{ t('channels.test.storedHint') }}</p>
          <TestSend
            :args="testArgs"
            :send="channelsResource.test"
          />
        </Panel>
      </div>

      <Panel
        id="channel-deliveries"
        :title="t('channels.deliveries.title')"
        :meta="t('channels.deliveries.meta')"
        flush
      >
        <div class="border-t border-hairline">
          <DeliveryTable
            :deliveries="rows"
            :label="t('channels.deliveries.title')"
            :error="deliveries.error.value && !rows ? message(deliveries.error.value) : null"
            :empty="t('channels.deliveries.empty')"
          />
        </div>
        <div
          v-if="more"
          class="flex items-center gap-3 border-t border-hairline p-3"
        >
          <Button
            v-if="limit < MAX"
            variant="fill"
            size="sm"
            :loading="deliveries.loading.value"
            @click="limit = Math.min(limit + PAGE, MAX)"
          >
            {{ t('channels.deliveries.showOlder') }}
          </Button>
          <p
            v-else
            class="text-sm text-muted"
          >
            {{ t('channels.deliveries.limit', { count: MAX }) }}
          </p>
        </div>
      </Panel>
    </div>
  </div>
</template>
