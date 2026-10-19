<script setup lang="ts">
import ChoiceGroup from '~/components/ChoiceGroup.vue'
import Icon from '~/components/Icon.vue'
import PagePreviewView from '~/components/pages/PagePreviewView.vue'
import SelectField from '~/components/SelectField.vue'
import { Alert } from '~/components/ui/alert'
import { Button } from '~/components/ui/button'
import { Skeleton } from '~/components/ui/skeleton'
import { useErrorMessage } from '~/composables/useErrorMessage'
import { usePageEditor, usePagePreview } from '~/composables/usePages'
import type { Locale, PageAudience } from '~/generated'
import { localeNames } from '~/lib/pages'
import { useI18n } from '~/plugins/i18n'

/**
 * The page as the public or the team sees it, in one of its languages, read from the server
 * so it holds exactly what that audience gets. Read again when the page changes or on request.
 */
const { t } = useI18n()
const { message } = useErrorMessage()
const editor = usePageEditor()
const page = computed(() => editor.data.value?.page ?? null)

const audience = ref<PageAudience>('public')
const locales = computed(() => page.value?.locales ?? [page.value?.default_locale ?? 'en'])
const locale = ref<Locale>(page.value?.default_locale ?? 'en')
watch(locales, (list) => {
  if (!list.includes(locale.value)) locale.value = list[0] ?? 'en'
})

const preview = usePagePreview(
  () => page.value?.id ?? '',
  audience,
  locale,
  () => page.value?.revision ?? 0
)
const audienceOptions = computed(() => [
  { value: 'public' as const, label: t('pages.preview.public') },
  { value: 'internal' as const, label: t('pages.preview.internal') },
])
const localeOptions = computed(() => locales.value.map((value) => ({ value, label: localeNames[value] })))
</script>

<template>
  <div
    v-if="page"
    class="grid gap-5"
  >
    <div class="flex flex-wrap items-end gap-4">
      <div class="grid gap-2">
        <p class="text-sm font-medium text-primary">{{ t('pages.preview.audience') }}</p>
        <ChoiceGroup
          v-model="audience"
          :label="t('pages.preview.audience')"
          :options="audienceOptions"
        />
      </div>
      <div
        v-if="localeOptions.length > 1"
        class="w-40"
      >
        <SelectField
          id="preview-locale"
          v-model="locale"
          :label="t('pages.preview.locale')"
          :options="localeOptions"
        />
      </div>
      <Button
        type="button"
        variant="ghost"
        class="ml-auto"
        :loading="preview.isFetching.value"
        @click="preview.refetch()"
      >
        <Icon name="lucide:refresh-cw" />
        {{ t('pages.preview.refresh') }}
      </Button>
    </div>
    <p class="text-sm text-muted">
      {{ audience === 'public' ? t('pages.preview.publicHint') : t('pages.preview.internalHint') }}
    </p>

    <Alert
      v-if="preview.error.value && !preview.data.value"
      color="destructive"
    >
      <p class="text-sm">{{ message(preview.error.value) }}</p>
    </Alert>
    <Skeleton
      v-else-if="!preview.data.value"
      class="h-64 w-full rounded-panel"
    />
    <PagePreviewView
      v-else
      :snapshot="preview.data.value.snapshot"
    />
  </div>
</template>
