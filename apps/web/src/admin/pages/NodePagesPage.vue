<script setup lang="ts">
import Icon from '~/components/Icon.vue'
import LiveIndicator from '~/components/LiveIndicator.vue'
import PageHeader from '~/components/PageHeader.vue'
import CreatePageDialog from '~/components/pages/CreatePageDialog.vue'
import { Alert } from '~/components/ui/alert'
import { Badge } from '~/components/ui/badge'
import { Button } from '~/components/ui/button'
import { EmptyState } from '~/components/ui/empty-state'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableNone, TableRow } from '~/components/ui/table'
import TableEmptyRow from '~/components/ui/TableEmptyRow.vue'
import TableLoadingRow from '~/components/ui/TableLoadingRow.vue'
import { useErrorMessage } from '~/composables/useErrorMessage'
import { useCurrentNode, useNodeDetail } from '~/composables/useNodes'
import { usePageMeta } from '~/composables/usePageMeta'
import { useNodePages } from '~/composables/usePages'
import { PAGE_PATH } from '~/generated/limits'
import { can } from '~/lib/access'
import { useI18n } from '~/plugins/i18n'

/**
 * Status pages on a node: title and address, who can open them, custom domains, and a link to
 * the page itself. Admins create pages here; each opens in its editor.
 */
const { t } = useI18n()
const { message } = useErrorMessage()
const { routeId } = useCurrentNode()
const detail = useNodeDetail(routeId)
const node = computed(() => detail.data.value?.node ?? null)
const manage = computed(() => can(node.value, 'pages.manage'))
usePageMeta(() => t('pages.title'))

const live = useNodePages(routeId)
const rows = computed(() => live.data.value?.pages ?? [])
const creating = ref(false)

const loading = computed(() => !live.data.value && !live.error.value)
const columns = 5
</script>

<template>
  <div>
    <PageHeader
      :title="t('pages.title')"
      :description="node?.name"
    >
      <template
        v-if="manage"
        #actions
      >
        <Button
          variant="accent"
          @click="creating = true"
        >
          <Icon name="lucide:plus" />
          {{ t('pages.create') }}
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
      class="max-w-5xl"
    >
      <p class="text-sm">{{ message(detail.error.value) }}</p>
    </Alert>
    <div
      v-else
      class="grid max-w-5xl gap-4"
    >
      <p class="text-sm text-muted">{{ t('pages.intro') }}</p>

      <EmptyState
        v-if="live.data.value && rows.length === 0"
        icon="lucide:globe"
        :title="t('pages.empty')"
        :description="manage ? t('pages.emptyHint') : t('pages.emptyReadOnly')"
      >
        <Button
          v-if="manage"
          variant="accent"
          @click="creating = true"
        >
          <Icon name="lucide:plus" />
          {{ t('pages.create') }}
        </Button>
      </EmptyState>

      <Table
        v-else
        :label="t('pages.title')"
      >
        <TableHeader>
          <TableRow>
            <TableHead>{{ t('pages.columns.page') }}</TableHead>
            <TableHead>{{ t('pages.columns.access') }}</TableHead>
            <TableHead class="max-md:hidden">{{ t('pages.columns.components') }}</TableHead>
            <TableHead class="max-md:hidden">{{ t('pages.columns.hostnames') }}</TableHead>
            <TableHead><span class="sr-only">{{ t('actions.label') }}</span></TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          <TableLoadingRow
            v-if="loading"
            :colspan="columns"
            :rows="2"
          />
          <TableEmptyRow
            v-else-if="live.error.value && !live.data.value"
            :colspan="columns"
            :label="message(live.error.value)"
          />
          <TableRow
            v-for="page in rows"
            :key="page.id"
            :data-page="page.slug"
          >
            <TableCell
              kind="subject"
              class="max-w-0 min-w-40"
            >
              <RouterLink
                :to="{ name: 'node-page', params: { nodeId: page.node, pageId: page.id } }"
                class="block truncate rounded-sm font-medium text-primary underline-offset-4 hover:underline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring"
              >
                {{ page.title }}
              </RouterLink>
              <span class="block truncate text-xs font-normal text-muted">{{ PAGE_PATH }}{{ page.slug }}</span>
            </TableCell>
            <TableCell>
              <Badge
                :variant="page.public ? 'success' : 'surface'"
                size="sm"
                :data-access="page.public ? 'public' : 'restricted'"
              >
                {{ page.public ? t('pages.access.modes.public') : t('pages.access.restricted') }}
              </Badge>
            </TableCell>
            <TableCell
              kind="secondary"
              class="tabular-nums max-md:hidden"
            >
              {{ page.components }}
            </TableCell>
            <TableCell
              kind="secondary"
              class="max-w-0 max-md:hidden"
            >
              <span
                v-if="page.hostnames.length > 0"
                class="block truncate"
                :title="page.hostnames.join(', ')"
              >{{ page.hostnames.join(', ') }}</span>
              <TableNone v-else />
            </TableCell>
            <TableCell class="text-right whitespace-nowrap">
              <Button
                as-child
                variant="ghost"
                size="icon"
              >
                <a
                  :href="`${PAGE_PATH}${page.slug}`"
                  target="_blank"
                  rel="noopener"
                  :aria-label="t('pages.openNamed', { title: page.title })"
                >
                  <Icon name="lucide:external-link" />
                </a>
              </Button>
            </TableCell>
          </TableRow>
        </TableBody>
      </Table>
    </div>

    <CreatePageDialog
      v-if="routeId && manage"
      v-model:open="creating"
      :node="routeId"
    />
  </div>
</template>
