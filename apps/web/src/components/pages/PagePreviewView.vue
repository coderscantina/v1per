<script setup lang="ts">
import Icon from '~/components/Icon.vue'
import IncidentStateBadge from '~/components/incidents/IncidentStateBadge.vue'
import { Badge } from '~/components/ui/badge'
import { useDateFormat } from '~/composables/useDateFormat'
import type { PageSnapshot } from '~/generated'
import { pageStatusFill, pageStatusLabels } from '~/lib/pages'
import { useI18n } from '~/plugins/i18n'

/**
 * A read-only outline of what a page shows one audience: overall status, maintenance,
 * published incidents and each visible component with its status and 90-day uptime. The page
 * itself, with bars and theming, opens from the editor's header.
 */
defineProps<{ snapshot: PageSnapshot }>()

const { t } = useI18n()
const { dateTime } = useDateFormat()
const percent = (ratio: number) => `${(ratio * 100).toFixed(2)} %`
</script>

<template>
  <div
    class="grid gap-5"
    data-testid="page-preview"
    :data-audience="snapshot.audience"
  >
    <div class="flex flex-wrap items-center gap-3 rounded-panel border border-hairline bg-card p-4">
      <span
        class="size-3 shrink-0 rounded-full"
        :class="pageStatusFill[snapshot.status]"
        aria-hidden="true"
      />
      <div class="grid min-w-0 flex-1 gap-0.5">
        <p class="truncate text-base font-semibold text-primary">{{ snapshot.appearance.title }}</p>
        <p
          class="text-sm text-muted"
          :data-status="snapshot.status"
        >
          {{ t(pageStatusLabels[snapshot.status]) }}
        </p>
      </div>
    </div>

    <p
      v-if="snapshot.description"
      class="text-sm whitespace-pre-line text-foreground"
    >
      {{ snapshot.description }}
    </p>

    <section
      v-if="snapshot.maintenance.length > 0"
      class="grid gap-2"
      aria-labelledby="preview-maintenance"
    >
      <h3
        id="preview-maintenance"
        class="text-sm font-medium text-primary"
      >
        {{ t('pages.preview.maintenance') }}
      </h3>
      <ul class="grid gap-1.5 text-sm">
        <li
          v-for="(entry, index) in snapshot.maintenance"
          :key="index"
          class="flex flex-wrap items-center gap-2"
        >
          <Icon
            name="lucide:wrench"
            size="14"
            class="text-muted"
            aria-hidden="true"
          />
          <span class="text-primary">{{ entry.title }}</span>
          <span class="text-muted">
            {{ entry.active ? t('pages.preview.active') : entry.starts_at ? dateTime(entry.starts_at) : '' }}
          </span>
        </li>
      </ul>
    </section>

    <section
      class="grid gap-2"
      aria-labelledby="preview-incidents"
    >
      <h3
        id="preview-incidents"
        class="text-sm font-medium text-primary"
      >
        {{ t('pages.preview.incidents') }}
      </h3>
      <ul
        v-if="snapshot.incidents.length > 0"
        class="grid gap-1.5 text-sm"
      >
        <li
          v-for="incident in snapshot.incidents"
          :key="incident.id"
          class="flex flex-wrap items-center gap-2"
          :data-incident="incident.title"
        >
          <IncidentStateBadge :state="incident.state" />
          <span class="text-primary">{{ incident.title }}</span>
          <span class="text-muted">{{ dateTime(incident.started_at) }}</span>
        </li>
      </ul>
      <p
        v-else
        class="text-sm text-muted"
      >
        {{ t('pages.preview.noIncidents') }}
      </p>
    </section>

    <section
      v-for="group in snapshot.groups"
      :key="group.id"
      class="overflow-hidden rounded-panel border border-hairline bg-card"
      :aria-label="group.name || t('pages.layout.untitled')"
    >
      <h3
        v-if="group.name"
        class="border-b border-hairline px-4 py-2.5 text-sm font-semibold text-primary"
      >
        {{ group.name }}
      </h3>
      <ul class="divide-y divide-divider">
        <li
          v-for="component in group.components"
          :key="component.id"
          class="flex flex-wrap items-center gap-x-3 gap-y-1 px-4 py-2.5 text-sm"
          :data-preview-component="component.name"
        >
          <span class="min-w-0 flex-1 truncate text-primary">{{ component.name }}</span>
          <Badge
            v-if="component.detail"
            variant="surface"
            size="sm"
          >
            {{ t('pages.preview.withDetail') }}
          </Badge>
          <span
            v-if="component.uptime_90d !== undefined"
            class="text-muted tabular-nums"
          >{{ percent(component.uptime_90d) }}</span>
          <span
            v-if="component.status"
            class="inline-flex items-center gap-2 whitespace-nowrap text-foreground"
          >
            <span
              class="size-2 rounded-full"
              :class="pageStatusFill[component.status]"
              aria-hidden="true"
            />
            {{ t(pageStatusLabels[component.status]) }}
          </span>
          <span
            v-else
            class="text-muted"
          >{{ t('pages.preview.noStatus') }}</span>
        </li>
      </ul>
    </section>

    <p
      v-if="snapshot.groups.length === 0"
      class="text-sm text-muted"
    >
      {{ t('pages.preview.noComponents') }}
    </p>

    <p
      v-if="snapshot.footer"
      class="border-t border-divider pt-3 text-xs whitespace-pre-line text-muted"
    >
      {{ snapshot.footer }}
    </p>
  </div>
</template>
