<script setup lang="ts">
import { Alert } from '~/components/ui/alert'
import { Button } from '~/components/ui/button'
import { EmptyState } from '~/components/ui/empty-state'
import { Skeleton } from '~/components/ui/skeleton'
import { useErrorMessage } from '~/composables/useErrorMessage'
import { useCurrentNode } from '~/composables/useNodes'
import { usePageMeta } from '~/composables/usePageMeta'
import { useI18n } from '~/plugins/i18n'

/**
 * Where the app opens: the last node visited, or the highest one the caller reads. Someone
 * whose grants were all removed sees that instead of an empty shell.
 */
const { t } = useI18n()
const { message } = useErrorMessage()
const router = useRouter()
const { node, tree } = useCurrentNode()
usePageMeta(() => t('home.title'))

watch(
  node,
  (next) => {
    if (next) void router.replace({ name: 'node', params: { nodeId: next.id } })
  },
  { immediate: true }
)
</script>

<template>
  <div class="mx-auto grid max-w-lg gap-4 pt-8">
    <Alert
      v-if="tree.error.value && !tree.data.value"
      color="destructive"
    >
      <div class="flex flex-wrap items-center justify-between gap-3">
        <p class="text-sm">{{ message(tree.error.value) }}</p>
        <Button
          size="sm"
          variant="outline"
          @click="tree.reload()"
        >
          {{ t('actions.retry') }}
        </Button>
      </div>
    </Alert>
    <EmptyState
      v-else-if="tree.data.value && !node"
      icon="lucide:folder-lock"
      :title="t('home.noAccess')"
      :description="t('home.noAccessHint')"
    />
    <Skeleton
      v-else
      class="h-24 w-full rounded-panel"
    />
  </div>
</template>
