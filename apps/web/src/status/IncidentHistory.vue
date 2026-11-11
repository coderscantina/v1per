<script setup lang="ts">
import type { PageClient } from './client'
import { formatMonth } from './format'
import IncidentMeta from './IncidentMeta.vue'
import { locale, t } from './messages'
import type { PageRouter } from './route'
import StatusIcon from './StatusIcon.vue'
import { toneIcon, toneLabel } from './tones'
import { byMonth, incidentView, type IncidentView } from './view'

/**
 * Resolved incidents, newest first and grouped by month, read from `incidents.json` ten at a
 * time. A row says how bad it got, when, for how long and where; its title opens the incident's
 * own page with the full timeline and postmortem. A change in `watch` (the snapshot's incidents
 * changed) starts over from the newest page.
 */
const props = defineProps<{
  client: PageClient
  names: ReadonlyMap<string, string>
  watch: string
  now: number
  route: PageRouter
}>()

const PAGE = 10
const incidents = shallowRef<IncidentView[]>([])
const months = computed(() => byMonth(incidents.value))
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
    <div
      v-else-if="incidents.length > 0"
      class="flex flex-col gap-5"
    >
      <div
        v-for="month in months"
        :key="month.key"
        class="flex flex-col gap-2"
      >
        <h3 class="text-sm font-medium text-muted">{{ formatMonth(month.at, locale) }}</h3>
        <ul class="divide-y divide-divider rounded-panel border border-border bg-card shadow-soft-sm">
          <li
            v-for="incident in month.items"
            :key="incident.id"
            :data-incident="incident.title"
            class="relative flex items-start gap-3 px-4 py-3 transition-colors duration-100 first:rounded-t-panel last:rounded-b-panel hover:bg-fill"
          >
            <span class="flex min-w-0 flex-1 flex-col gap-0.5">
              <span class="flex flex-wrap items-start justify-between gap-x-3 gap-y-0.5">
                <a
                  :href="route.incidentHref(incident.id)"
                  class="min-w-0 rounded-sm font-medium text-primary after:absolute after:inset-0"
                  @click="route.openIncident(incident.id, $event)"
                >{{ incident.title }}</a>
                <span
                  v-if="incident.peak"
                  :data-tone="incident.peak"
                  data-testid="incident-impact"
                  class="inline-flex shrink-0 items-center gap-1.5 text-sm text-foreground"
                >
                  <StatusIcon
                    :name="toneIcon[incident.peak]"
                    class="size-4 text-(--tone)"
                  />
                  {{ t(toneLabel[incident.peak]) }}
                </span>
              </span>
              <IncidentMeta
                :incident="incident"
                :now="now"
                components
              >
                <template v-if="incident.postmortem">
                  <span class="font-medium text-foreground">{{ t('incident.postmortem') }}</span>
                  ·
                </template>
              </IncidentMeta>
            </span>
            <StatusIcon
              name="chevron"
              class="mt-0.5 size-4 shrink-0 -rotate-90 text-muted"
            />
          </li>
        </ul>
      </div>
    </div>
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
