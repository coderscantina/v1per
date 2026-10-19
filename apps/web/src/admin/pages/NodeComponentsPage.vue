<script setup lang="ts">
import { useMutation } from '@tanstack/vue-query'

import { componentsResource } from '~/api/resources/components'
import Icon from '~/components/Icon.vue'
import LiveIndicator from '~/components/LiveIndicator.vue'
import PageHeader from '~/components/PageHeader.vue'
import ComponentDialog from '~/components/static/ComponentDialog.vue'
import { Alert } from '~/components/ui/alert'
import { Button } from '~/components/ui/button'
import { EmptyState } from '~/components/ui/empty-state'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableNone, TableRow } from '~/components/ui/table'
import TableEmptyRow from '~/components/ui/TableEmptyRow.vue'
import TableLoadingRow from '~/components/ui/TableLoadingRow.vue'
import { useNodeComponents } from '~/composables/useComponents'
import { useConfirm } from '~/composables/useConfirm'
import { useErrorMessage } from '~/composables/useErrorMessage'
import { useCurrentNode, useNodeDetail } from '~/composables/useNodes'
import { usePageMeta } from '~/composables/usePageMeta'
import type { Component } from '~/generated'
import { can } from '~/lib/access'
import { useI18n } from '~/plugins/i18n'

/**
 * Static components of a node: status page entries without a monitor, such as "Support" or
 * "Billing". Incidents name them like monitors; status pages place them later. Admins add,
 * rename and delete them.
 */
const { t } = useI18n()
const { message } = useErrorMessage()
const { confirm } = useConfirm()
const { routeId } = useCurrentNode()
const detail = useNodeDetail(routeId)
const node = computed(() => detail.data.value?.node ?? null)
const manage = computed(() => can(node.value, 'pages.manage'))
usePageMeta(() => t('components.title'))

const live = useNodeComponents(routeId)
const rows = computed(() => live.data.value?.components ?? [])

const editing = ref<Component | null>(null)
const dialogOpen = ref(false)
const openDialog = (component: Component | null) => {
  editing.value = component
  dialogOpen.value = true
}

const notice = ref<string | null>(null)
const remove = useMutation({
  mutationFn: (component: Component) => componentsResource.delete(component.id),
  onMutate: () => (notice.value = null),
  onSuccess: (_, component) => (notice.value = t('components.deleted', { name: component.name })),
})
const ask = async (component: Component) => {
  const ok = await confirm({
    title: t('components.deleteTitle'),
    message: t('components.deleteConfirm', { name: component.name }),
    confirmLabel: t('components.delete'),
    variant: 'destructive',
  })
  if (ok) remove.mutate(component)
}

const loading = computed(() => !live.data.value && !live.error.value)
const columns = 3
</script>

<template>
  <div>
    <PageHeader
      :title="t('components.title')"
      :description="node?.name"
    >
      <template
        v-if="manage"
        #actions
      >
        <Button
          variant="accent"
          @click="openDialog(null)"
        >
          <Icon name="lucide:plus" />
          {{ t('components.create') }}
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
      class="max-w-4xl"
    >
      <p class="text-sm">{{ message(detail.error.value) }}</p>
    </Alert>
    <div
      v-else
      class="grid max-w-4xl gap-4"
    >
      <p class="text-sm text-muted">{{ t('components.intro') }}</p>
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
        v-if="live.data.value && rows.length === 0"
        icon="lucide:box"
        :title="t('components.empty')"
        :description="manage ? t('components.emptyHint') : t('components.emptyReadOnly')"
      >
        <Button
          v-if="manage"
          variant="accent"
          @click="openDialog(null)"
        >
          <Icon name="lucide:plus" />
          {{ t('components.create') }}
        </Button>
      </EmptyState>

      <Table
        v-else
        :label="t('components.title')"
      >
        <TableHeader>
          <TableRow>
            <TableHead>{{ t('components.name') }}</TableHead>
            <TableHead class="max-sm:hidden">{{ t('components.description') }}</TableHead>
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
            v-for="component in rows"
            :key="component.id"
            :data-component="component.name"
          >
            <TableCell kind="subject">{{ component.name }}</TableCell>
            <TableCell
              kind="secondary"
              class="max-w-0 max-sm:hidden"
            >
              <span
                v-if="component.description"
                class="block truncate"
                :title="component.description"
              >{{ component.description }}</span>
              <TableNone v-else />
            </TableCell>
            <TableCell class="text-right whitespace-nowrap">
              <template v-if="manage">
                <Button
                  variant="ghost"
                  size="icon"
                  :aria-label="t('components.editNamed', { name: component.name })"
                  @click="openDialog(component)"
                >
                  <Icon name="lucide:pencil" />
                </Button>
                <Button
                  variant="ghost"
                  size="icon"
                  :disabled="remove.isPending.value"
                  :aria-label="t('components.deleteNamed', { name: component.name })"
                  @click="ask(component)"
                >
                  <Icon name="lucide:trash-2" />
                </Button>
              </template>
            </TableCell>
          </TableRow>
        </TableBody>
      </Table>
    </div>

    <ComponentDialog
      v-if="routeId"
      v-model:open="dialogOpen"
      :node="routeId"
      :component="editing"
    />
  </div>
</template>
