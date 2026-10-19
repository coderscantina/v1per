<script setup lang="ts">
import type { TreeNode } from '~/generated'
import { useI18n } from '~/plugins/i18n'

/** A node's ancestors as links, slash separated. Ancestors the caller cannot read show as "…". */
defineProps<{ path: TreeNode[] }>()

const { t } = useI18n()
</script>

<template>
  <span class="flex min-w-0 flex-wrap items-center gap-x-1.5">
    <template
      v-for="(entry, at) in path"
      :key="entry.id"
    >
      <span
        v-if="at > 0"
        class="text-faint"
        aria-hidden="true"
      >/</span>
      <RouterLink
        v-if="entry.name !== undefined"
        :to="{ name: 'node', params: { nodeId: entry.id } }"
        class="rounded-sm text-primary underline-offset-4 hover:underline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring"
      >
        {{ entry.name }}
      </RouterLink>
      <span
        v-else
        class="text-muted"
      >
        <span aria-hidden="true">…</span>
        <span class="sr-only">{{ t('nodes.hidden') }}</span>
      </span>
    </template>
  </span>
</template>
