<script setup lang="ts">
import type { PageSnapshot } from '~/generated'

import type { PageClient } from './client'
import ComponentGroup from './ComponentGroup.vue'
import IncidentCard from './IncidentCard.vue'
import IncidentHistory from './IncidentHistory.vue'
import IncidentPage from './IncidentPage.vue'
import LocaleSwitch from './LocaleSwitch.vue'
import MaintenanceCard from './MaintenanceCard.vue'
import { t } from './messages'
import PageFigures from './PageFigures.vue'
import { usePageRange } from './range'
import RangeSwitch from './RangeSwitch.vue'
import { usePageRoute, viewLabel, views } from './route'
import SlaCard from './SlaCard.vue'
import StatusBanner from './StatusBanner.vue'
import SubscribeDialog from './SubscribeDialog.vue'
import UptimeRules from './UptimeRules.vue'
import { pageFigures, pageView } from './view'

/** Where "Powered by" points. */
const REPOSITORY = 'https://github.com/coderscantina/v1per'

const props = defineProps<{ snapshot: PageSnapshot; base: string; client: PageClient }>()

const view = computed(() => pageView(props.snapshot, props.base))
const route = usePageRoute(props.base)
const underWay = computed(() => view.value.maintenance.filter((window) => window.active))
const { range, options: rangeOptions, pick } = usePageRange(() => props.snapshot.range_max)

// The clock for "5 minutes ago" and running durations, ticking once a minute.
const now = ref(Date.now())
let tick: ReturnType<typeof setInterval> | undefined
onMounted(() => (tick = setInterval(() => (now.value = Date.now()), 60_000)))
onBeforeUnmount(() => clearInterval(tick))
const figures = computed(() => pageFigures(view.value, range.value, now.value))
const incidentsKey = computed(() =>
  props.snapshot.incidents
    .map((incident) => `${incident.id}:${incident.state}:${incident.updates.length}:${incident.postmortem?.published_at ?? ''}`)
    .join()
)
</script>

<template>
  <div class="mx-auto flex min-h-svh max-w-3xl flex-col gap-8 px-4 pt-8 pb-8 sm:px-6 sm:pt-10">
    <header class="flex flex-col gap-4">
      <div class="flex flex-col gap-4 sm:flex-row sm:items-center sm:justify-between">
        <div class="flex min-w-0 items-center gap-3">
          <img
            v-if="view.logo"
            :src="view.logo"
            alt=""
            class="h-8 w-auto max-w-40 object-contain"
          >
          <h1 class="truncate text-xl font-semibold tracking-tight text-primary">{{ view.title }}</h1>
        </div>
        <div class="flex flex-wrap items-center gap-2 self-start sm:self-auto">
          <nav
            :aria-label="t('nav.label')"
            class="flex rounded-control bg-fill p-[3px]"
          >
            <a
              v-for="item in views"
              :key="item"
              :href="route.href(item)"
              :aria-current="item === route.current.value ? 'page' : undefined"
              class="rounded-lg px-3 py-1 text-sm font-medium text-muted transition-colors duration-100 hover:text-primary aria-[current]:bg-card aria-[current]:text-primary aria-[current]:shadow-[0_0_0_1px_var(--border-strong)]"
              @click="route.open(item, $event)"
            >{{ t(viewLabel[item]) }}</a>
          </nav>
          <SubscribeDialog
            v-if="snapshot.subscriptions && (snapshot.subscriptions.email || snapshot.subscriptions.feeds)"
            :offer="snapshot.subscriptions"
            :names="view.names"
            :base="base"
            :client="client"
          />
        </div>
      </div>
      <template v-if="route.current.value === 'status'">
        <p
          v-if="view.description"
          class="whitespace-pre-line text-muted"
        >{{ view.description }}</p>
        <p
          v-if="view.audience === 'internal'"
          class="flex flex-wrap items-center gap-2 text-sm text-muted"
          data-testid="internal-note"
        >
          <span class="rounded-md bg-fill px-2 py-0.5 text-xs font-medium text-primary">{{ t('internal.badge') }}</span>
          {{ t('internal.hint') }}
        </p>
      </template>
    </header>

    <main
      id="main-content"
      class="flex flex-col gap-8"
    >
      <template v-if="route.current.value === 'status'">
        <StatusBanner
          :status="view.status"
          :generated-at="view.generatedAt"
        />

        <PageFigures
          :figures="figures"
          :range="range"
        />

        <section
          v-if="view.active.length > 0 || underWay.length > 0"
          aria-labelledby="active-incidents"
          class="flex flex-col gap-3"
        >
          <h2
            id="active-incidents"
            class="sr-only"
          >{{ t('incidents.active') }}</h2>
          <IncidentCard
            v-for="incident in view.active"
            :key="incident.id"
            :incident="incident"
            :now="now"
            :route="route"
          />
          <MaintenanceCard
            v-for="(window, index) in underWay"
            :key="index"
            :maintenance="window"
          />
        </section>

        <section
          v-if="view.groups.length > 0"
          aria-labelledby="components"
          class="flex flex-col gap-3"
        >
          <div class="flex flex-wrap items-center justify-between gap-3">
            <h2
              id="components"
              class="text-lg font-semibold text-primary"
            >{{ t('components.title') }}</h2>
            <RangeSwitch
              :model-value="range"
              :options="rangeOptions"
              @update:model-value="pick"
            />
          </div>
          <ComponentGroup
            v-for="group in view.groups"
            :key="group.id"
            :group="group"
            :range="range"
          />
        </section>

        <section
          v-if="view.slas.length > 0"
          aria-labelledby="slas"
          class="flex flex-col gap-3"
        >
          <h2
            id="slas"
            class="text-lg font-semibold text-primary"
          >{{ t('sla.title') }}</h2>
          <SlaCard
            v-for="(sla, index) in view.slas"
            :key="index"
            :sla="sla"
          />
        </section>
      </template>

      <section
        v-else-if="route.current.value === 'maintenance'"
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
        <p
          v-if="view.maintenance.length === 0"
          class="text-sm text-muted"
        >{{ t('maintenance.none') }}</p>
      </section>

      <IncidentPage
        v-else-if="route.incident.value !== null"
        :id="route.incident.value"
        :incidents="snapshot.incidents"
        :names="view.names"
        :client="client"
        :route="route"
        :watch="snapshot.cursor"
        :now="now"
      />

      <IncidentHistory
        v-else
        :client="client"
        :names="view.names"
        :watch="incidentsKey"
        :now="now"
        :route="route"
      />
    </main>

    <footer class="mt-auto flex flex-col gap-3 border-t border-border pt-6 text-sm text-muted">
      <p
        v-if="view.footer"
        class="whitespace-pre-line"
      >{{ view.footer }}</p>
      <UptimeRules v-if="route.current.value === 'status'" />
      <div class="flex flex-wrap items-center justify-between gap-3">
        <LocaleSwitch :locales="snapshot.appearance.locales" />
        <p class="ml-auto">
          {{ t('footer.poweredBy') }}
          <a
            :href="REPOSITORY"
            class="rounded-sm font-medium text-foreground underline-offset-4 hover:text-primary hover:underline"
          >V1Per</a>
        </p>
      </div>
    </footer>
  </div>
</template>
