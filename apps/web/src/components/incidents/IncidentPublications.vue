<script setup lang="ts">
import Icon from '~/components/Icon.vue'
import { Alert } from '~/components/ui/alert'
import { useDateFormat } from '~/composables/useDateFormat'
import { useErrorMessage } from '~/composables/useErrorMessage'
import type { IncidentPublication, IncidentPublications } from '~/generated'
import { PAGE_PATH } from '~/generated/limits'
import { cn } from '~/lib/utils'
import { useI18n } from '~/plugins/i18n'

/**
 * The status pages that show what an incident affects, or carry it already, as one row of
 * chips. For members a chip publishes the incident there or takes it down; others see the pages
 * it is on. Nothing renders while no page could carry it.
 */
const props = defineProps<{
  publications: IncidentPublications | undefined
  manage: boolean
  pending: boolean
  error: unknown
}>()
const emit = defineEmits<{ publish: [page: IncidentPublication]; unpublish: [page: IncidentPublication] }>()

const { t } = useI18n()
const { dateTime } = useDateFormat()
const { message } = useErrorMessage()

const shown = computed(() =>
  (props.publications?.pages ?? []).filter((entry) => props.manage || entry.published_at !== undefined)
)
const published = (entry: IncidentPublication) => entry.published_at !== undefined
const status = (entry: IncidentPublication) =>
  entry.published_at !== undefined
    ? t('incidents.pages.publishedAt', { date: dateTime(entry.published_at) })
    : entry.scheduled_at !== undefined
      ? t('incidents.pages.scheduledAt', { date: dateTime(entry.scheduled_at) })
      : t('incidents.pages.draft')
const toggle = (entry: IncidentPublication) => (published(entry) ? emit('unpublish', entry) : emit('publish', entry))
</script>

<template>
  <section
    v-if="shown.length > 0 || error"
    class="grid gap-2"
    :aria-label="t('incidents.pages.title')"
  >
    <div class="flex flex-wrap items-center gap-1.5">
      <span class="mr-1 text-sm text-muted">{{ t('incidents.pages.title') }}</span>
      <span
        v-for="entry in shown"
        :key="entry.page"
        class="inline-flex items-center"
        :data-publication="entry.slug"
        :data-published="published(entry)"
      >
        <button
          v-if="manage"
          type="button"
          :class="
            cn(
              'inline-flex h-7 cursor-pointer items-center gap-1.5 rounded-l-md px-2.5 text-sm font-medium transition-colors duration-200 ease-butter focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring disabled:opacity-50',
              published(entry) ? 'bg-fill-strong text-primary hover:bg-fill' : 'border border-r-0 border-dashed border-border-strong text-muted hover:text-primary'
            )
          "
          :disabled="pending"
          :title="status(entry)"
          :aria-pressed="published(entry)"
          :aria-label="published(entry) ? t('incidents.pages.unpublishFrom', { title: entry.title }) : t('incidents.pages.publishTo', { title: entry.title })"
          @click="toggle(entry)"
        >
          <Icon
            :name="published(entry) ? 'lucide:check' : entry.scheduled_at !== undefined ? 'lucide:bell' : 'lucide:plus'"
            size="13"
            aria-hidden="true"
          />
          {{ entry.title }}
        </button>
        <span
          v-else
          class="inline-flex h-7 items-center rounded-l-md bg-fill px-2.5 text-sm text-primary"
          :title="status(entry)"
        >{{ entry.title }}</span>
        <a
          :href="`${PAGE_PATH}${entry.slug}`"
          target="_blank"
          rel="noopener"
          :class="
            cn(
              'grid h-7 w-7 place-items-center rounded-r-md text-muted transition-colors duration-200 ease-butter hover:text-primary focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring',
              !manage ? 'bg-fill' : published(entry) ? 'bg-fill-strong' : 'border border-dashed border-border-strong'
            )
          "
          :aria-label="t('pages.openNamed', { title: entry.title })"
        >
          <Icon
            name="lucide:external-link"
            size="12"
          />
        </a>
      </span>
    </div>
    <Alert
      v-if="error"
      color="destructive"
    >
      <p class="text-sm">{{ message(error) }}</p>
    </Alert>
  </section>
</template>
