<script setup lang="ts">
// Adapted from holocron (84fc54ec): apps/web/src/pages/account/ApiKeysPage.vue, with a node and
// capabilities per key.
import { useMutation, useQuery } from '@tanstack/vue-query'

import { queryKeys } from '~/api/query-keys'
import { accountResource } from '~/api/resources/account'
import ApiKeyDialog from '~/components/account/ApiKeyDialog.vue'
import SettingsPage from '~/components/account/SettingsPage.vue'
import SettingsSection from '~/components/account/SettingsSection.vue'
import Icon from '~/components/Icon.vue'
import { Alert } from '~/components/ui/alert'
import { Badge } from '~/components/ui/badge'
import { Button } from '~/components/ui/button'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '~/components/ui/table'
import TableEmptyRow from '~/components/ui/TableEmptyRow.vue'
import TableLoadingRow from '~/components/ui/TableLoadingRow.vue'
import { useConfirm } from '~/composables/useConfirm'
import { useDateFormat } from '~/composables/useDateFormat'
import { useErrorMessage } from '~/composables/useErrorMessage'
import { useNodeTree } from '~/composables/useNodes'
import { usePageMeta } from '~/composables/usePageMeta'
import type { ApiKey } from '~/generated'
import { API_KEY_SCHEME } from '~/generated/limits'
import { pathTo } from '~/lib/node-tree'
import { capabilityLabels } from '~/lib/roles'
import { useI18n } from '~/plugins/i18n'
import { queryClient } from '~/plugins/vue-query'

/**
 * Scoped API keys. A key acts as its owner on one node and its subtree, limited to its
 * capabilities; the owner's current role still applies on top, so a key only ever does less.
 */
const { t } = useI18n()
const { message } = useErrorMessage()
const { confirm } = useConfirm()
const { date, dateTime } = useDateFormat()
const tree = useNodeTree()
usePageMeta(() => t('apiKeys.title'))

const keys = useQuery({ queryKey: queryKeys.apiKeys, queryFn: ({ signal }) => accountResource.apiKeys(signal) })
const creating = ref(false)

const revoke = useMutation({
  mutationFn: (key: ApiKey) => accountResource.revokeApiKey(key.id),
  onSuccess: () => queryClient.invalidateQueries({ queryKey: queryKeys.apiKeys }),
})

const ask = async (key: ApiKey) => {
  const ok = await confirm({
    title: t('apiKeys.revoke'),
    message: t('apiKeys.revokeConfirm', { name: key.name }),
    confirmLabel: t('apiKeys.revoke'),
    variant: 'destructive',
  })
  if (ok) revoke.mutate(key)
}

const nodePath = (key: ApiKey) => {
  const path = pathTo(tree.index.value, key.node)
  return path.length > 0 ? path.map((entry) => entry.name ?? '…').join(' / ') : t('apiKeys.nodeGone')
}

const COLUMNS = 5
</script>

<template>
  <SettingsPage>
    <SettingsSection
      id="keys"
      :title="t('apiKeys.list')"
      :description="t('apiKeys.description', { scheme: API_KEY_SCHEME })"
    >
      <Alert
        v-if="revoke.error.value"
        color="destructive"
      >
        <p class="text-sm">{{ message(revoke.error.value) }}</p>
      </Alert>
      <Table :label="t('apiKeys.list')">
        <TableHeader>
          <TableRow>
            <TableHead>{{ t('apiKeys.name') }}</TableHead>
            <TableHead>{{ t('apiKeys.node') }}</TableHead>
            <TableHead>{{ t('apiKeys.capabilities') }}</TableHead>
            <TableHead>{{ t('apiKeys.lastUsed') }}</TableHead>
            <TableHead><span class="sr-only">{{ t('actions.label') }}</span></TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          <TableLoadingRow
            v-if="keys.isPending.value"
            :colspan="COLUMNS"
            :rows="3"
          />
          <TableEmptyRow
            v-else-if="(keys.data.value?.keys ?? []).length === 0"
            :colspan="COLUMNS"
            :label="keys.isError.value ? message(keys.error.value) : t('apiKeys.empty')"
          />
          <TableRow
            v-for="key in keys.data.value?.keys ?? []"
            :key="key.id"
          >
            <TableCell>
              <div class="grid min-w-0 gap-0.5">
                <span
                  class="truncate font-medium"
                  :class="key.revoked_at ? 'text-muted' : 'text-primary'"
                >{{ key.name }}</span>
                <span class="font-mono text-xs text-muted">{{ key.prefix }}…</span>
              </div>
            </TableCell>
            <TableCell class="min-w-32 text-muted">{{ nodePath(key) }}</TableCell>
            <TableCell>
              <div class="flex max-w-xs flex-wrap gap-1">
                <Badge
                  v-for="capability in key.capabilities"
                  :key="capability"
                  size="sm"
                >
                  {{ t(capabilityLabels[capability]) }}
                </Badge>
              </div>
            </TableCell>
            <TableCell class="whitespace-nowrap text-muted">
              <Badge
                v-if="key.revoked_at"
                variant="destructive"
                size="sm"
              >
                {{ t('apiKeys.revokedOn', { date: date(key.revoked_at) }) }}
              </Badge>
              <span
                v-else-if="key.last_used_at"
                :title="dateTime(key.last_used_at)"
              >
                {{ date(key.last_used_at) }}
              </span>
              <span v-else>{{ t('apiKeys.neverUsed') }}</span>
            </TableCell>
            <TableCell class="text-right">
              <Button
                v-if="!key.revoked_at"
                variant="ghost"
                size="icon"
                :aria-label="t('apiKeys.revokeNamed', { name: key.name })"
                @click="ask(key)"
              >
                <Icon name="lucide:trash-2" />
              </Button>
            </TableCell>
          </TableRow>
        </TableBody>
      </Table>
      <template #footer>
        <Button
          variant="fill"
          @click="creating = true"
        >
          <Icon name="lucide:plus" />
          {{ t('apiKeys.create') }}
        </Button>
      </template>
    </SettingsSection>
    <ApiKeyDialog v-model:open="creating" />
  </SettingsPage>
</template>
