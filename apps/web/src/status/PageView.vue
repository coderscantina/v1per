<script setup lang="ts">
import type { PageSnapshot } from '~/generated'

import type { PageClient } from './client'
import ComponentGroup from './ComponentGroup.vue'
import IncidentCard from './IncidentCard.vue'
import IncidentHistory from './IncidentHistory.vue'
import LocaleSwitch from './LocaleSwitch.vue'
import MaintenanceCard from './MaintenanceCard.vue'
import { t } from './messages'
import StatusBanner from './StatusBanner.vue'
import { pageView } from './view'

const props = defineProps<{ snapshot: PageSnapshot; base: string; client: PageClient }>()

const view = computed(() => pageView(props.snapshot, props.base))
const incidentsKey = computed(() =>
  props.snapshot.incidents.map((incident) => `${incident.id}:${incident.state}:${incident.updates.length}`).join()
)
</script>

<template>
  <div class="mx-auto flex min-h-svh max-w-3xl flex-col gap-8 px-4 pt-10 pb-8 sm:px-6 sm:pt-14">
    <header class="flex flex-col gap-4">
      <img
        v-if="view.logo"
        :src="view.logo"
        alt=""
        class="h-10 w-auto max-w-48 self-start object-contain"
      >
      <div class="flex flex-col gap-2">
        <h1 class="text-2xl font-semibold tracking-tight text-primary sm:text-3xl">{{ view.title }}</h1>
        <p
          v-if="view.description"
          class="whitespace-pre-line text-muted"
        >{{ view.description }}</p>
      </div>
      <p
        v-if="view.audience === 'internal'"
        class="flex flex-wrap items-center gap-2 text-sm text-muted"
        data-testid="internal-note"
      >
        <span class="rounded-md bg-fill px-2 py-0.5 text-xs font-medium text-primary">{{ t('internal.badge') }}</span>
        {{ t('internal.hint') }}
      </p>
    </header>

    <main
      id="main-content"
      class="flex flex-col gap-8"
    >
      <StatusBanner
        :status="view.status"
        :generated-at="view.generatedAt"
      />

      <section
        v-if="view.active.length > 0"
        aria-labelledby="active-incidents"
        class="flex flex-col gap-3"
      >
        <h2
          id="active-incidents"
          class="text-lg font-semibold text-primary"
        >{{ t('incidents.active') }}</h2>
        <IncidentCard
          v-for="incident in view.active"
          :key="incident.id"
          :incident="incident"
        />
      </section>

      <section
        v-if="view.maintenance.length > 0"
        aria-labelledby="maintenance"
        class="flex flex-col gap-3"
      >
        <h2
          id="maintenance"
          class="text-lg font-semibold text-primary"
        >{{ t('maintenance.title') }}</h2>
        <MaintenanceCard
          v-for="(window, index) in view.maintenance"
          :key="index"
          :maintenance="window"
        />
      </section>

      <section
        v-if="view.groups.length > 0"
        aria-labelledby="components"
        class="flex flex-col gap-3"
      >
        <h2
          id="components"
          class="sr-only"
        >{{ t('components.title') }}</h2>
        <ComponentGroup
          v-for="group in view.groups"
          :key="group.id"
          :group="group"
        />
      </section>

      <IncidentHistory
        :client="client"
        :names="view.names"
        :watch="incidentsKey"
      />
    </main>

    <footer class="mt-auto flex flex-col gap-3 border-t border-border pt-6 text-sm text-muted">
      <p
        v-if="view.footer"
        class="whitespace-pre-line"
      >{{ view.footer }}</p>
      <div class="flex flex-wrap items-center justify-between gap-3">
        <LocaleSwitch :locales="snapshot.appearance.locales" />
        <span>{{ t('footer.poweredBy') }}</span>
      </div>
    </footer>
  </div>
</template>
