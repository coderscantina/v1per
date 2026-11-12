<script setup lang="ts">
import type { PublicIncident } from '~/generated'

import AffectedList from './AffectedList.vue'
import type { PageClient } from './client'
import { formatDate } from './format'
import IncidentMeta from './IncidentMeta.vue'
import { locale, t } from './messages'
import type { PageRouter } from './route'
import StatusIcon from './StatusIcon.vue'
import { stateLabel, toneIcon, toneLabel } from './tones'
import UpdateTimeline from './UpdateTimeline.vue'
import { incidentView } from './view'

/**
 * One incident on its own page: title, impact, state, when and for how long, what it affects,
 * the postmortem and every update. It reads from the snapshot while that lists the incident, and
 * from `incidents/<id>.json` for an older one, again whenever the page's topic moves (`watch`).
 */
const props = defineProps<{
  id: string
  incidents: readonly PublicIncident[]
  names: ReadonlyMap<string, string>
  client: PageClient
  route: PageRouter
  watch: string
  now: number
}>()

const listed = computed(() => props.incidents.find((incident) => incident.id === props.id))
const fetched = shallowRef<PublicIncident | null>(null)
const failed = ref(false)
let generation = 0

async function load() {
  if (listed.value) return
  const mine = ++generation
  try {
    const incident = await props.client.incident(props.id)
    if (mine !== generation) return
    fetched.value = incident
    failed.value = false
  } catch {
    if (mine === generation) failed.value = true
  }
}

watch(
  () => props.id,
  () => {
    fetched.value = null
    failed.value = false
  }
)
watch([() => props.id, () => props.watch], load, { immediate: true })

const incident = computed(() => {
  const raw = listed.value ?? (fetched.value?.id === props.id ? fetched.value : null)
  return raw ? incidentView(raw, props.names) : null
})
/** Open: how bad it is now. Resolved: how bad it got. */
const impact = computed(() => (incident.value?.state === 'resolved' ? incident.value.peak : incident.value?.impact))
const paragraphs = computed(() => incident.value?.postmortem?.text.split(/\n\s*\n/) ?? [])
</script>

<template>
  <section class="flex flex-col gap-6">
    <a
      :href="route.href('incidents')"
      class="inline-flex items-center gap-1 self-start rounded-sm text-sm font-medium text-accent-text"
      @click="route.open('incidents', $event)"
    >
      <StatusIcon
        name="chevron"
        class="size-4 rotate-90"
      />
      {{ t('incident.back') }}
    </a>

    <article
      v-if="incident"
      :data-incident="incident.title"
      class="flex flex-col gap-6"
    >
      <header class="flex flex-col gap-2">
        <h2 class="text-2xl font-semibold tracking-tight text-primary">{{ incident.title }}</h2>
        <p class="flex flex-wrap items-center gap-x-2 gap-y-1 text-sm">
          <span
            v-if="impact"
            :data-tone="impact"
            data-testid="incident-impact"
            class="inline-flex items-center gap-1.5 font-medium text-primary"
          >
            <StatusIcon
              :name="toneIcon[impact]"
              class="size-4 text-(--tone)"
            />
            {{ t(toneLabel[impact]) }}
          </span>
          <span
            v-if="impact"
            class="text-muted"
            aria-hidden="true"
          >·</span>
          <span
            class="font-medium text-primary"
            data-testid="incident-state"
          >{{ t(stateLabel[incident.state]) }}</span>
        </p>
        <IncidentMeta
          :incident="incident"
          :now="now"
        />
      </header>

      <AffectedList :affected="incident.affected" />

      <section
        v-if="incident.postmortem"
        aria-labelledby="postmortem"
        data-testid="postmortem"
        class="flex flex-col gap-3 rounded-panel border border-border bg-card p-4 shadow-soft-sm sm:p-5"
      >
        <header class="flex flex-wrap items-baseline justify-between gap-x-3 gap-y-0.5">
          <h3
            id="postmortem"
            class="font-semibold text-primary"
          >{{ t('incident.postmortem') }}</h3>
          <time
            class="text-sm text-muted"
            :datetime="new Date(incident.postmortem.published_at).toISOString()"
          >{{ t('incident.postmortemAt', { date: formatDate(incident.postmortem.published_at, locale) }) }}</time>
        </header>
        <p
          v-for="(paragraph, index) in paragraphs"
          :key="index"
          class="whitespace-pre-line text-foreground"
        >{{ paragraph }}</p>
      </section>

      <section
        aria-labelledby="incident-updates"
        class="flex flex-col gap-3"
      >
        <h3
          id="incident-updates"
          class="font-semibold text-primary"
        >{{ t('incidents.updates') }}</h3>
        <UpdateTimeline :entries="incident.timeline" />
      </section>
    </article>

    <p
      v-else-if="failed"
      role="alert"
      class="text-sm text-destructive-foreground"
    >{{ t('incident.missing') }}</p>
    <p
      v-else
      role="status"
      class="text-sm text-muted"
    >{{ t('incident.loading') }}</p>
  </section>
</template>
