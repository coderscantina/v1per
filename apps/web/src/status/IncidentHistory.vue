<script setup lang="ts">
import AffectedList from './AffectedList.vue'
import { formatDuration, formatTime } from './format'
import type { PageClient } from './client'
import { locale, t } from './messages'
import StatusIcon from './StatusIcon.vue'
import UpdateTimeline from './UpdateTimeline.vue'
import { incidentView, type IncidentView } from './view'

/**
 * Resolved incidents, newest first, read from `incidents.json` ten at a time. A change in
 * `watch` (the snapshot's incidents changed) starts over from the newest page.
 */
const props = defineProps<{
  client: PageClient
  names: ReadonlyMap<string, string>
  watch: string
}>()

const PAGE = 10
const incidents = shallowRef<IncidentView[]>([])
const next = ref<string | undefined>(undefined)
const loaded = ref(false)
const loading = ref(false)
const failed = ref(false)
let generation = 0

/** Loads the page after `cursor`; no cursor starts over. */
async function load(cursor?: string) {
  const mine = ++generation
  loading.value = true
  failed.value = false
  try {
    const page = await props.client.incidents(PAGE, cursor)
    if (mine !== generation) return
    const resolved = page.incidents
      .filter((incident) => incident.state === 'resolved')
      .map((incident) => incidentView(incident, props.names))
    incidents.value = cursor ? [...incidents.value, ...resolved] : resolved
    next.value = page.next_cursor
    loaded.value = true
  } catch {
    if (mine !== generation) return
    failed.value = true
  } finally {
    if (mine === generation) loading.value = false
  }
}

watch(() => props.watch, () => load(), { immediate: true })
</script>

<template>
  <section
    aria-labelledby="incident-history"
    class="flex flex-col gap-3"
  >
    <h2
      id="incident-history"
      class="text-lg font-semibold text-primary"
    >{{ t('incidents.history') }}</h2>
    <p
      v-if="loaded && incidents.length === 0 && next === undefined"
      class="text-sm text-muted"
    >{{ t('incidents.none') }}</p>
    <ul
      v-else-if="incidents.length > 0"
      class="divide-y divide-divider rounded-panel border border-border bg-card shadow-soft-sm"
    >
      <li
        v-for="incident in incidents"
        :key="incident.id"
        :data-incident="incident.title"
      >
        <details class="group/details">
          <summary class="flex cursor-pointer list-none items-start gap-3 rounded-panel px-4 py-3 [&::-webkit-details-marker]:hidden">
            <StatusIcon
              name="chevron"
              class="mt-0.5 size-4 shrink-0 -rotate-90 text-muted transition-transform group-open/details:rotate-0"
            />
            <span class="flex min-w-0 flex-1 flex-col gap-0.5">
              <span class="font-medium text-primary">{{ incident.title }}</span>
              <span class="text-sm text-muted">
                <time :datetime="new Date(incident.startedAt).toISOString()">{{ formatTime(incident.startedAt, locale) }}</time>
                <template v-if="incident.resolvedAt !== undefined"> · {{ t('incidents.resolvedAfter', { duration: formatDuration(incident.resolvedAt - incident.startedAt) }) }}</template>
              </span>
            </span>
          </summary>
          <div class="flex flex-col gap-3 px-4 pb-4 pl-11">
            <AffectedList :affected="incident.affected" />
            <UpdateTimeline :updates="incident.updates" />
          </div>
        </details>
      </li>
    </ul>
    <p
      v-if="failed"
      role="alert"
      class="text-sm text-destructive-foreground"
    >{{ t('incidents.error') }}</p>
    <p
      v-else-if="loading && !loaded"
      role="status"
      class="text-sm text-muted"
    >{{ t('incidents.loading') }}</p>
    <button
      v-if="failed || (next !== undefined && loaded)"
      type="button"
      class="self-start rounded-control border border-border bg-card px-3 py-1.5 text-sm font-medium text-primary shadow-soft-sm hover:bg-fill disabled:opacity-60"
      :disabled="loading"
      @click="load(failed && !loaded ? undefined : next)"
    >
      {{ loading ? t('incidents.loading') : failed ? t('incidents.retry') : t('incidents.more') }}
    </button>
  </section>
</template>
