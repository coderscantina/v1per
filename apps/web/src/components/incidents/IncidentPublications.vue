<script setup lang="ts">
import Icon from '~/components/Icon.vue'
import { Alert } from '~/components/ui/alert'
import { Button } from '~/components/ui/button'
import { Panel } from '~/components/ui/panel'
import { Skeleton } from '~/components/ui/skeleton'
import { useDateFormat } from '~/composables/useDateFormat'
import { useErrorMessage } from '~/composables/useErrorMessage'
import type { IncidentPublication, IncidentPublications } from '~/generated'
import { PAGE_PATH } from '~/generated/limits'
import { useI18n } from '~/plugins/i18n'

/**
 * The status pages that show what an incident affects, or carry it already, each with whether
 * the incident is published there and, for members, a switch to publish or take it down.
 */
defineProps<{
  publications: IncidentPublications | undefined
  manage: boolean
  pending: boolean
  error: unknown
}>()
const emit = defineEmits<{ publish: [page: IncidentPublication]; unpublish: [page: IncidentPublication] }>()

const { t } = useI18n()
const { dateTime } = useDateFormat()
const { message } = useErrorMessage()
</script>

<template>
  <Panel
    id="incident-pages"
    :title="t('incidents.pages.title')"
    :meta="publications ? String(publications.pages.length) : undefined"
    flush
  >
    <Skeleton
      v-if="!publications"
      class="mx-4.5 mb-4.5 h-12"
    />
    <ul
      v-else-if="publications.pages.length > 0"
      class="divide-y divide-divider border-t border-hairline"
    >
      <li
        v-for="entry in publications.pages"
        :key="entry.page"
        class="flex flex-wrap items-center gap-x-3 gap-y-1 px-4.5 py-2.5"
        :data-publication="entry.slug"
        :data-published="entry.published_at !== undefined"
      >
        <div class="grid min-w-0 flex-1 basis-48 gap-0.5">
          <a
            :href="`${PAGE_PATH}${entry.slug}`"
            target="_blank"
            rel="noopener"
            class="inline-flex min-w-0 items-center gap-1.5 truncate rounded-sm text-sm font-medium text-primary underline-offset-4 hover:underline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring"
          >
            <span class="truncate">{{ entry.title }}</span>
            <Icon
              name="lucide:external-link"
              size="12"
              class="shrink-0 text-muted"
              aria-hidden="true"
            />
          </a>
          <span class="text-xs text-muted">
            {{
              entry.published_at !== undefined
                ? t('incidents.pages.publishedAt', { date: dateTime(entry.published_at) })
                : entry.scheduled_at !== undefined
                  ? t('incidents.pages.scheduledAt', { date: dateTime(entry.scheduled_at) })
                  : t('incidents.pages.draft')
            }}
          </span>
        </div>
        <Button
          v-if="manage"
          type="button"
          size="sm"
          :variant="entry.published_at !== undefined ? 'ghost' : 'fill'"
          :disabled="pending"
          :aria-label="
            entry.published_at !== undefined
              ? t('incidents.pages.unpublishFrom', { title: entry.title })
              : t('incidents.pages.publishTo', { title: entry.title })
          "
          @click="entry.published_at !== undefined ? emit('unpublish', entry) : emit('publish', entry)"
        >
          {{ entry.published_at !== undefined ? t('incidents.pages.unpublish') : t('incidents.pages.publish') }}
        </Button>
      </li>
    </ul>
    <p
      v-else
      class="border-t border-hairline px-4.5 py-6 text-sm text-muted"
    >
      {{ t('incidents.pages.none') }}
    </p>
    <Alert
      v-if="error"
      color="destructive"
      class="m-3 w-auto"
    >
      <p class="text-sm">{{ message(error) }}</p>
    </Alert>
  </Panel>
</template>
