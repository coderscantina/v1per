<script setup lang="ts">
import { useMutation } from '@tanstack/vue-query'

import { channelsResource } from '~/api/resources/channels'
import Icon from '~/components/Icon.vue'
import LiveIndicator from '~/components/LiveIndicator.vue'
import PageHeader from '~/components/PageHeader.vue'
import { Alert } from '~/components/ui/alert'
import { Badge } from '~/components/ui/badge'
import { Button } from '~/components/ui/button'
import { EmptyState } from '~/components/ui/empty-state'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '~/components/ui/table'
import TableEmptyRow from '~/components/ui/TableEmptyRow.vue'
import TableLoadingRow from '~/components/ui/TableLoadingRow.vue'
import { useAvailableChannels, useNodeChannels } from '~/composables/useChannels'
import { useConfirm } from '~/composables/useConfirm'
import { useErrorMessage } from '~/composables/useErrorMessage'
import { useCurrentNode, useNodeDetail } from '~/composables/useNodes'
import { usePageMeta } from '~/composables/usePageMeta'
import type { Channel } from '~/generated'
import { can } from '~/lib/access'
import { providerIcon, providerName, providerOf } from '~/lib/channels'
import { useI18n } from '~/plugins/i18n'

/**
 * Where a node's alerts go: its own channels, with their settings and delivery logs for admins,
 * and the channels of the nodes above, which its monitors may use too and which are managed
 * there. Default channels join every new monitor on the node and below.
 */
const { t } = useI18n()
const { message } = useErrorMessage()
const { confirm } = useConfirm()
const { routeId } = useCurrentNode()
const detail = useNodeDetail(routeId)
const node = computed(() => detail.data.value?.node ?? null)
const manage = computed(() => can(node.value, 'channels.manage'))
usePageMeta(() => t('channels.title'))

/** The settings and delivery logs need `channels.manage`; everyone else sees the choices. */
const own = useNodeChannels(() => (manage.value ? routeId.value : null))
const available = useAvailableChannels(routeId)
const channels = computed(() => own.data.value?.channels ?? [])
const inherited = computed(() => (available.data.value?.channels ?? []).filter((channel) => !channel.own))
const ownChoices = computed(() => (available.data.value?.channels ?? []).filter((channel) => channel.own))
const live = computed(() => (manage.value ? own : available))

const notice = ref<string | null>(null)
const remove = useMutation({
  mutationFn: (channel: Channel) => channelsResource.delete(channel.id),
  onMutate: () => (notice.value = null),
  onSuccess: (_, channel) => (notice.value = t('channels.deleted', { name: channel.name })),
})
const ask = async (channel: Channel) => {
  const ok = await confirm({
    title: t('channels.deleteTitle'),
    message:
      channel.monitors > 0
        ? t('channels.deleteConfirmUsed', { name: channel.name, count: channel.monitors }, channel.monitors)
        : t('channels.deleteConfirm', { name: channel.name }),
    confirmLabel: t('channels.delete'),
    variant: 'destructive',
  })
  if (ok) remove.mutate(channel)
}

const loading = computed(() => (manage.value ? !own.data.value && !own.error.value : !available.data.value && !available.error.value))
const empty = computed(() =>
  manage.value
    ? own.data.value !== null && channels.value.length === 0 && inherited.value.length === 0
    : available.data.value !== null && (available.data.value?.channels.length ?? 0) === 0
)
const columns = 5
</script>

<template>
  <div>
    <PageHeader
      :title="t('channels.title')"
      :description="node?.name"
    >
      <template
        v-if="manage && routeId"
        #actions
      >
        <Button
          as-child
          variant="accent"
        >
          <RouterLink :to="{ name: 'node-channel-new', params: { nodeId: routeId } }">
            <Icon name="lucide:plus" />
            {{ t('channels.create') }}
          </RouterLink>
        </Button>
      </template>
      <template
        v-if="live.data.value"
        #freshness
      >
        <LiveIndicator :status="live.status.value" />
      </template>
    </PageHeader>

    <Alert
      v-if="detail.error.value && !node"
      color="destructive"
      class="max-w-6xl"
    >
      <p class="text-sm">{{ message(detail.error.value) }}</p>
    </Alert>
    <div
      v-else
      class="grid max-w-6xl gap-8"
    >
      <Alert
        v-if="node && !manage"
        icon="lucide:info"
      >
        <p class="text-sm">{{ t('channels.readOnly') }}</p>
      </Alert>
      <div
        aria-live="polite"
        class="empty:hidden"
      >
        <Alert
          v-if="remove.error.value"
          color="destructive"
        >
          <p class="text-sm">{{ message(remove.error.value) }}</p>
        </Alert>
        <Alert
          v-else-if="notice"
          color="success"
        >
          <p class="text-sm">{{ notice }}</p>
        </Alert>
      </div>

      <EmptyState
        v-if="empty"
        icon="lucide:bell"
        :title="t('channels.empty')"
        :description="manage ? t('channels.emptyHint') : t('channels.emptyReadOnly')"
      >
        <Button
          v-if="manage && routeId"
          as-child
          variant="accent"
        >
          <RouterLink :to="{ name: 'node-channel-new', params: { nodeId: routeId } }">
            <Icon name="lucide:plus" />
            {{ t('channels.create') }}
          </RouterLink>
        </Button>
      </EmptyState>

      <template v-else>
        <section
          class="grid gap-3"
          aria-labelledby="channels-own-title"
        >
          <h2
            id="channels-own-title"
            class="flex items-center gap-2.5 text-[1.0625rem] font-semibold text-primary"
          >
            {{ t('channels.own') }}
            <span
              v-if="live.data.value"
              class="text-sm font-normal text-muted tabular-nums"
            >· {{ manage ? channels.length : ownChoices.length }}</span>
          </h2>
          <Table :label="t('channels.own')">
            <TableHeader>
              <TableRow>
                <TableHead>{{ t('channels.columns.name') }}</TableHead>
                <TableHead>{{ t('channels.columns.provider') }}</TableHead>
                <TableHead class="max-sm:hidden">{{ t('channels.columns.flags') }}</TableHead>
                <TableHead
                  v-if="manage"
                  numeric
                  class="max-sm:hidden"
                >
                  {{ t('channels.columns.monitors') }}
                </TableHead>
                <TableHead v-if="manage"><span class="sr-only">{{ t('actions.label') }}</span></TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              <TableLoadingRow
                v-if="loading"
                :colspan="columns"
                :rows="2"
              />
              <TableEmptyRow
                v-else-if="live.error.value"
                :colspan="columns"
                :label="message(live.error.value)"
              />
              <TableEmptyRow
                v-else-if="(manage ? channels.length : ownChoices.length) === 0"
                :colspan="columns"
                :label="manage ? t('channels.noneOwn') : t('channels.noneOwnReadOnly')"
              />
              <template v-if="manage">
                <TableRow
                  v-for="channel in channels"
                  :key="channel.id"
                  :data-channel="channel.name"
                >
                  <TableCell
                    kind="subject"
                    class="max-w-0 min-w-44"
                  >
                    <RouterLink
                      :to="{ name: 'node-channel', params: { nodeId: channel.node, channelId: channel.id } }"
                      class="block truncate rounded-sm underline-offset-4 hover:underline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring"
                    >
                      {{ channel.name }}
                    </RouterLink>
                  </TableCell>
                  <TableCell
                    kind="secondary"
                    class="whitespace-nowrap"
                    :label="t('channels.columns.provider')"
                  >
                    <span class="inline-flex items-center gap-1.5">
                      <Icon
                        :name="providerIcon(providerOf(channel.settings))"
                        size="14"
                        aria-hidden="true"
                      />
                      {{ providerName(providerOf(channel.settings), t) }}
                    </span>
                  </TableCell>
                  <TableCell class="max-sm:hidden">
                    <div class="flex flex-wrap gap-1">
                      <Badge
                        v-if="channel.is_default"
                        variant="secondary"
                        size="sm"
                      >
                        {{ t('channels.default') }}
                      </Badge>
                      <Badge
                        v-if="!channel.active"
                        variant="warning"
                        size="sm"
                        :title="channel.inactive_reason"
                      >
                        {{ t('channels.inactive') }}
                      </Badge>
                    </div>
                  </TableCell>
                  <TableCell
                    kind="numeric"
                    class="max-sm:hidden"
                    :label="t('channels.columns.monitors')"
                  >
                    {{ channel.monitors }}
                  </TableCell>
                  <TableCell class="text-right whitespace-nowrap">
                    <Button
                      v-if="channel.settings.type !== 'unsupported'"
                      as-child
                      variant="ghost"
                      size="icon"
                    >
                      <RouterLink
                        :to="{ name: 'node-channel-edit', params: { nodeId: channel.node, channelId: channel.id } }"
                        :aria-label="t('channels.editNamed', { name: channel.name })"
                      >
                        <Icon name="lucide:pencil" />
                      </RouterLink>
                    </Button>
                    <Button
                      variant="ghost"
                      size="icon"
                      :aria-label="t('channels.deleteNamed', { name: channel.name })"
                      :disabled="remove.isPending.value"
                      @click="ask(channel)"
                    >
                      <Icon name="lucide:trash-2" />
                    </Button>
                  </TableCell>
                </TableRow>
              </template>
              <template v-else>
                <TableRow
                  v-for="channel in ownChoices"
                  :key="channel.id"
                  :data-channel="channel.name"
                >
                  <TableCell kind="subject">{{ channel.name }}</TableCell>
                  <TableCell
                    kind="secondary"
                    :label="t('channels.columns.provider')"
                  >
                    {{ providerName(channel.provider, t) }}
                  </TableCell>
                  <TableCell class="max-sm:hidden">
                    <div class="flex flex-wrap gap-1">
                      <Badge
                        v-if="channel.is_default"
                        variant="secondary"
                        size="sm"
                      >
                        {{ t('channels.default') }}
                      </Badge>
                      <Badge
                        v-if="!channel.active"
                        variant="warning"
                        size="sm"
                      >
                        {{ t('channels.inactive') }}
                      </Badge>
                    </div>
                  </TableCell>
                </TableRow>
              </template>
            </TableBody>
          </Table>
        </section>

        <section
          v-if="inherited.length > 0"
          class="grid gap-3"
          aria-labelledby="channels-inherited-title"
        >
          <div class="grid gap-1">
            <h2
              id="channels-inherited-title"
              class="flex items-center gap-2.5 text-[1.0625rem] font-semibold text-primary"
            >
              {{ t('channels.inherited') }}
              <span class="text-sm font-normal text-muted tabular-nums">· {{ inherited.length }}</span>
            </h2>
            <p class="text-sm text-muted">{{ t('channels.inheritedHint') }}</p>
          </div>
          <Table :label="t('channels.inherited')">
            <TableHeader>
              <TableRow>
                <TableHead>{{ t('channels.columns.name') }}</TableHead>
                <TableHead>{{ t('channels.columns.provider') }}</TableHead>
                <TableHead class="max-sm:hidden">{{ t('channels.columns.flags') }}</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              <TableRow
                v-for="channel in inherited"
                :key="channel.id"
                :data-channel="channel.name"
              >
                <TableCell kind="subject">
                  <span class="inline-flex flex-wrap items-center gap-2">
                    {{ channel.name }}
                    <Badge
                      variant="surface"
                      size="sm"
                    >
                      {{ t('channels.inheritedBadge') }}
                    </Badge>
                  </span>
                </TableCell>
                <TableCell
                  kind="secondary"
                  :label="t('channels.columns.provider')"
                >
                  {{ providerName(channel.provider, t) }}
                </TableCell>
                <TableCell class="max-sm:hidden">
                  <div class="flex flex-wrap gap-1">
                    <Badge
                      v-if="channel.is_default"
                      variant="secondary"
                      size="sm"
                    >
                      {{ t('channels.default') }}
                    </Badge>
                    <Badge
                      v-if="!channel.active"
                      variant="warning"
                      size="sm"
                    >
                      {{ t('channels.inactive') }}
                    </Badge>
                  </div>
                </TableCell>
              </TableRow>
            </TableBody>
          </Table>
        </section>
      </template>
    </div>
  </div>
</template>
