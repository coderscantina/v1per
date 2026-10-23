<script setup lang="ts">
import { useMutation } from '@tanstack/vue-query'

import { pagesResource } from '~/api/resources/pages'
import SettingsPage from '~/components/account/SettingsPage.vue'
import SettingsSection from '~/components/account/SettingsSection.vue'
import CommandAlert from '~/components/CommandAlert.vue'
import Icon from '~/components/Icon.vue'
import { Alert } from '~/components/ui/alert'
import { Button } from '~/components/ui/button'
import { Skeleton } from '~/components/ui/skeleton'
import { useNodeTree } from '~/composables/useNodes'
import { usePageEditor } from '~/composables/usePages'
import { useNodeSlas } from '~/composables/useSlas'
import type { PageDetail, Sla, SlaId } from '~/generated'
import { periodLabels } from '~/lib/sla-form'
import { useI18n } from '~/plugins/i18n'

/**
 * Which SLAs the page shows and in what order, from those on the page's node and below. Saved
 * on its own (`pages.updateSlas`) against the page revision the list was loaded at.
 */
const { t, locale } = useI18n()
const tree = useNodeTree()
const editor = usePageEditor()
const page = computed(() => editor.data.value?.page ?? null)
const manage = editor.manage

/** As many as the server accepts on one page. */
const PAGE_SLAS_MAX = 20

const slas = useNodeSlas(() => page.value?.node ?? null, true)
const byId = computed(() => new Map((slas.data.value?.slas ?? []).map((sla) => [sla.id, sla])))

const draft = ref<SlaId[]>([])
const basedOn = ref<number | null>(null)
const load = (detail: PageDetail) => {
  draft.value = [...detail.slas]
  basedOn.value = detail.page.revision
}
watch(
  () => editor.data.value,
  (detail) => {
    if (detail && basedOn.value === null) load(detail)
  },
  { immediate: true }
)

const shown = computed(() => draft.value.map((id) => byId.value.get(id)).filter((sla) => sla !== undefined))
const available = computed(() =>
  (slas.data.value?.slas ?? [])
    .filter((sla) => !draft.value.includes(sla.id))
    .sort((a, b) => a.name.localeCompare(b.name, locale.value))
)
const full = computed(() => draft.value.length >= PAGE_SLAS_MAX)

/** The period, and the node when the SLA sits below the page's own. */
const meta = (sla: Sla) => {
  const period = t(periodLabels[sla.period])
  const node = sla.node === page.value?.node ? null : tree.index.value.byId.get(sla.node)?.name
  return node ? `${node} · ${period}` : period
}

const saved = ref(false)
const dirty = computed(() => editor.data.value !== null && draft.value.join() !== editor.data.value.slas.join())
const show = (id: SlaId) => {
  saved.value = false
  draft.value = [...draft.value, id]
}
const hide = (id: SlaId) => {
  saved.value = false
  draft.value = draft.value.filter((entry) => entry !== id)
}
const move = (index: number, delta: number) => {
  const next = [...draft.value]
  const [entry] = next.splice(index, 1)
  if (entry === undefined) return
  next.splice(index + delta, 0, entry)
  saved.value = false
  draft.value = next
}

const save = useMutation({
  mutationFn: () => {
    const current = page.value
    if (!current) throw new Error('page not loaded')
    return pagesResource.updateSlas({ page: current.id, expected_revision: basedOn.value ?? undefined, slas: draft.value })
  },
  onSuccess: (detail) => {
    load(detail)
    saved.value = true
  },
})
const reload = () => {
  if (editor.data.value) load(editor.data.value)
  save.reset()
}
</script>

<template>
  <Alert
    v-if="!manage"
    icon="lucide:info"
  >
    <p class="text-sm">{{ t('pages.readOnly') }}</p>
  </Alert>
  <form
    v-else-if="page"
    novalidate
    @submit.prevent="save.mutate()"
  >
    <SettingsPage>
      <SettingsSection
        id="page-slas"
        :title="t('slas.page.title')"
        :description="t('slas.page.hint')"
      >
        <Skeleton
          v-if="!slas.data.value"
          class="h-32 w-full"
        />
        <p
          v-else-if="slas.data.value.slas.length === 0"
          class="text-sm text-muted"
        >
          {{ t('slas.page.noSlas') }}
        </p>
        <template v-else>
          <div class="grid gap-1.5">
            <p class="text-sm font-medium text-primary">{{ t('slas.page.shown') }}</p>
            <p
              v-if="shown.length === 0"
              class="text-sm text-muted"
            >
              {{ t('slas.page.noneShown') }}
            </p>
            <ol
              v-else
              class="divide-y divide-divider overflow-hidden rounded-control border border-hairline"
            >
              <li
                v-for="(sla, index) in shown"
                :key="sla.id"
                class="flex h-11 min-w-0 items-center gap-2 pr-1.5 pl-3 text-sm"
                :data-page-sla="sla.name"
              >
                <span class="w-5 shrink-0 text-muted tabular-nums">{{ index + 1 }}</span>
                <span class="min-w-0 flex-1 truncate text-primary">{{ sla.name }}</span>
                <span class="max-w-1/3 shrink-0 truncate text-muted max-sm:hidden">{{ meta(sla) }}</span>
                <Button
                  type="button"
                  variant="ghost"
                  size="icon"
                  :disabled="index === 0"
                  :aria-label="t('slas.page.moveUp', { name: sla.name })"
                  @click="move(index, -1)"
                >
                  <Icon name="lucide:arrow-up" />
                </Button>
                <Button
                  type="button"
                  variant="ghost"
                  size="icon"
                  :disabled="index === shown.length - 1"
                  :aria-label="t('slas.page.moveDown', { name: sla.name })"
                  @click="move(index, 1)"
                >
                  <Icon name="lucide:arrow-down" />
                </Button>
                <Button
                  type="button"
                  variant="ghost"
                  size="icon"
                  :aria-label="t('slas.page.hide', { name: sla.name })"
                  @click="hide(sla.id)"
                >
                  <Icon name="lucide:x" />
                </Button>
              </li>
            </ol>
          </div>
          <div class="grid gap-1.5">
            <p class="text-sm font-medium text-primary">{{ t('slas.page.available') }}</p>
            <p
              v-if="available.length === 0"
              class="text-sm text-muted"
            >
              {{ t('slas.page.noneAvailable') }}
            </p>
            <ul
              v-else
              class="divide-y divide-divider overflow-hidden rounded-control border border-hairline"
            >
              <li
                v-for="sla in available"
                :key="sla.id"
                class="flex h-11 min-w-0 items-center gap-2 pr-1.5 pl-3 text-sm"
              >
                <span class="min-w-0 flex-1 truncate text-primary">{{ sla.name }}</span>
                <span class="max-w-1/3 shrink-0 truncate text-muted max-sm:hidden">{{ meta(sla) }}</span>
                <Button
                  type="button"
                  variant="ghost"
                  size="icon"
                  :disabled="full"
                  :aria-label="t('slas.page.show', { name: sla.name })"
                  @click="show(sla.id)"
                >
                  <Icon name="lucide:plus" />
                </Button>
              </li>
            </ul>
            <p
              v-if="full && available.length > 0"
              class="text-sm text-muted"
            >
              {{ t('slas.page.limit', { max: PAGE_SLAS_MAX }) }}
            </p>
          </div>
        </template>
      </SettingsSection>
    </SettingsPage>

    <div class="mt-8 grid gap-4 border-t border-divider pt-6">
      <CommandAlert
        :error="save.error.value"
        @reload="reload"
      />
      <Alert
        v-if="saved && !save.error.value"
        color="success"
      >
        <p class="text-sm">{{ t('actions.saved') }}</p>
      </Alert>
      <div>
        <Button
          type="submit"
          variant="accent"
          :disabled="!dirty"
          :loading="save.isPending.value"
        >
          {{ t('slas.page.save') }}
        </Button>
      </div>
    </div>
  </form>
</template>
