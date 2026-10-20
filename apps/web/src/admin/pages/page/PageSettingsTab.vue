<script setup lang="ts">
import { useMutation } from '@tanstack/vue-query'

import { pagesResource } from '~/api/resources/pages'
import SettingsPage from '~/components/account/SettingsPage.vue'
import SettingsSection from '~/components/account/SettingsSection.vue'
import CheckboxGroup from '~/components/CheckboxGroup.vue'
import ChoiceGroup from '~/components/ChoiceGroup.vue'
import CommandAlert from '~/components/CommandAlert.vue'
import FormField from '~/components/FormField.vue'
import Icon from '~/components/Icon.vue'
import SelectField from '~/components/SelectField.vue'
import TextareaField from '~/components/TextareaField.vue'
import TimezoneField from '~/components/TimezoneField.vue'
import { Alert } from '~/components/ui/alert'
import { Button } from '~/components/ui/button'
import { useConfirm } from '~/composables/useConfirm'
import { useErrorMessage } from '~/composables/useErrorMessage'
import { usePageEditor } from '~/composables/usePages'
import type { Locale, PageDetail } from '~/generated'
import { PAGE_DESCRIPTION_MAX_CHARS, PAGE_FOOTER_MAX_CHARS, PAGE_LOGO_MAX_BYTES, PAGE_PATH, PAGE_SLUG_MAX_CHARS } from '~/generated/limits'
import {
  accents,
  autoIncidentModes,
  localeNames,
  pageLocales,
  settingsFromPage,
  themes,
  toUpdateArgs,
  validateSettings,
  type SettingsForm,
} from '~/lib/pages'
import { useI18n } from '~/plugins/i18n'

/**
 * A page's title, address, texts, logo and look, languages, bar timezone and what happens to
 * auto-incidents. Saved together against the revision the form was loaded at; the logo uploads
 * on its own. Deleting the page lives here too.
 */
const { t } = useI18n()
const { fieldMessage, fieldProblem, message } = useErrorMessage()
const { confirm } = useConfirm()
const router = useRouter()
const editor = usePageEditor()
const page = computed(() => editor.data.value?.page ?? null)
const manage = editor.manage

const form = ref<SettingsForm | null>(null)
const basedOn = ref<number | null>(null)
const load = (detail: PageDetail) => {
  form.value = settingsFromPage(detail.page)
  basedOn.value = detail.page.revision
}
watch(
  () => editor.data.value,
  (detail) => {
    if (detail && basedOn.value === null) load(detail)
  },
  { immediate: true }
)

const submitted = ref(false)
const clientErrors = computed(() => (submitted.value && form.value ? validateSettings(form.value) : {}))
const save = useMutation({
  mutationFn: ({ current, revision }: { current: SettingsForm; revision: number }) => {
    if (!page.value) throw new Error('page not loaded')
    return pagesResource.update(toUpdateArgs(current, page.value, revision))
  },
  onSuccess: (detail) => load(detail),
})
const errorFor = (key: string): string | null => {
  const local = clientErrors.value[key]
  return local ? fieldProblem(local) : fieldMessage(save.error.value, key)
}
const saved = ref(false)
const submit = () => {
  const current = form.value
  if (!current || basedOn.value === null) return
  submitted.value = true
  saved.value = false
  if (Object.keys(validateSettings(current)).length > 0) {
    void nextTick(() => document.querySelector<HTMLElement>('[aria-invalid="true"]')?.focus())
    return
  }
  save.mutate({ current, revision: basedOn.value }, { onSuccess: () => (saved.value = true) })
}
const reload = () => {
  if (editor.data.value) load(editor.data.value)
  save.reset()
}

const themeOptions = computed(() => themes.map((value) => ({ value, label: t(`pages.settings.themes.${value}`) })))
const accentOptions = computed(() => accents.map((value) => ({ value, label: t(`pages.settings.accents.${value}`) })))
const localeOptions = pageLocales.map((value) => ({ value, label: localeNames[value] }))
const enabledOptions = computed(() =>
  pageLocales.map((value) => ({
    value,
    label: localeNames[value],
    hint: value === form.value?.defaultLocale ? t('pages.settings.defaultAlwaysOn') : undefined,
    disabled: value === form.value?.defaultLocale,
  }))
)
const enabledLocales = computed({
  get: (): Locale[] => {
    const current = form.value
    if (!current) return []
    return pageLocales.filter((code) => code === current.defaultLocale || current.locales.includes(code))
  },
  set: (next: Locale[]) => {
    if (form.value) form.value.locales = next
  },
})
const modeOptions = computed(() => autoIncidentModes.map((value) => ({ value, label: t(`pages.settings.autoModes.${value}`) })))

const logoInput = ref<HTMLInputElement | null>(null)
const logoProblem = ref<string | null>(null)
/** The logo changes on its own; a form loaded just before it stays saveable. */
const afterLogo = (detail: PageDetail, before: number) => {
  if (basedOn.value === before) basedOn.value = detail.page.revision
}
const logo = useMutation({
  mutationFn: (file: File | null) => {
    const current = page.value
    if (!current) throw new Error('page not loaded')
    return file ? pagesResource.uploadLogo(current.id, file) : pagesResource.removeLogo(current.id)
  },
  onMutate: () => ({ before: page.value?.revision ?? 0 }),
  onSuccess: (detail, _, context) => afterLogo(detail, context.before),
})
const pickLogo = (event: Event) => {
  const input = event.target instanceof HTMLInputElement ? event.target : null
  const file = input?.files?.[0]
  if (input) input.value = ''
  if (!file) return
  logoProblem.value = file.size > PAGE_LOGO_MAX_BYTES ? t('pages.settings.logoTooLarge', { max: PAGE_LOGO_MAX_BYTES / 1024 }) : null
  if (!logoProblem.value) logo.mutate(file)
}

const remove = useMutation({ mutationFn: () => pagesResource.delete(page.value?.id ?? '') })
const askDelete = async () => {
  const current = page.value
  if (!current) return
  const ok = await confirm({
    title: t('pages.settings.deleteTitle'),
    message: t('pages.settings.deleteConfirm', { title: current.title }),
    confirmLabel: t('pages.settings.delete'),
    variant: 'destructive',
  })
  if (ok) remove.mutate(undefined, { onSuccess: () => void router.replace({ name: 'node-pages', params: { nodeId: current.node } }) })
}

const shownFields = ['title', 'slug', 'description', 'footer', 'timezone', 'grace_minutes']
</script>

<template>
  <Alert
    v-if="!manage"
    icon="lucide:info"
  >
    <p class="text-sm">{{ t('pages.readOnly') }}</p>
  </Alert>
  <form
    v-else-if="form && page"
    novalidate
    @submit.prevent="submit"
  >
    <SettingsPage>
      <SettingsSection
        id="page-basics"
        :title="t('pages.settings.basics')"
      >
        <FormField
          id="page-title"
          v-model="form.title"
          autocomplete="off"
          :label="t('pages.settings.title')"
          :error="errorFor('title')"
          required
        />
        <FormField
          id="page-slug"
          v-model="form.slug"
          autocomplete="off"
          :label="t('pages.settings.slug')"
          :description="t('pages.settings.slugHint', { path: `${PAGE_PATH}${form.slug || 'status'}` })"
          :maxlength="PAGE_SLUG_MAX_CHARS"
          :error="errorFor('slug')"
          required
        />
        <TextareaField
          id="page-description"
          v-model="form.description"
          :rows="3"
          :label="t('pages.settings.description')"
          :maxlength="PAGE_DESCRIPTION_MAX_CHARS"
          :error="errorFor('description')"
        />
        <TextareaField
          id="page-footer"
          v-model="form.footer"
          :rows="2"
          :label="t('pages.settings.footer')"
          :maxlength="PAGE_FOOTER_MAX_CHARS"
          :error="errorFor('footer')"
        />
      </SettingsSection>

      <SettingsSection
        id="page-look"
        :title="t('pages.settings.look')"
        :description="t('pages.settings.lookHint')"
      >
        <div class="grid gap-2">
          <p
            id="page-logo-label"
            class="text-sm font-medium text-primary"
          >
            {{ t('pages.settings.logo') }}
          </p>
          <div class="flex flex-wrap items-center gap-3">
            <div class="flex h-12 min-w-12 items-center justify-center rounded-lg border border-hairline bg-fill px-2">
              <img
                v-if="page.logo"
                :src="page.logo.url"
                :alt="t('pages.settings.logoAlt')"
                class="max-h-9 max-w-40 object-contain"
              >
              <Icon
                v-else
                name="lucide:image"
                class="text-muted"
                aria-hidden="true"
              />
            </div>
            <input
              ref="logoInput"
              type="file"
              class="sr-only"
              tabindex="-1"
              aria-labelledby="page-logo-label"
              accept="image/png,image/jpeg,image/gif,image/webp,image/svg+xml"
              @change="pickLogo"
            >
            <Button
              type="button"
              variant="fill"
              size="sm"
              :loading="logo.isPending.value"
              @click="logoInput?.click()"
            >
              <Icon name="lucide:upload" />
              {{ page.logo ? t('pages.settings.logoReplace') : t('pages.settings.logoUpload') }}
            </Button>
            <Button
              v-if="page.logo"
              type="button"
              variant="ghost"
              size="sm"
              :disabled="logo.isPending.value"
              @click="logo.mutate(null)"
            >
              {{ t('pages.settings.logoRemove') }}
            </Button>
          </div>
          <p
            v-if="logoProblem || logo.error.value"
            class="text-sm font-medium text-destructive"
            role="alert"
          >
            {{ logoProblem ?? (fieldMessage(logo.error.value, 'logo') ? t('pages.settings.logoInvalid') : message(logo.error.value)) }}
          </p>
        </div>
        <div class="grid gap-2">
          <p class="text-sm font-medium text-primary">{{ t('pages.settings.theme') }}</p>
          <ChoiceGroup
            v-model="form.theme"
            :label="t('pages.settings.theme')"
            :options="themeOptions"
            class="w-fit"
          />
        </div>
        <SelectField
          id="page-accent"
          v-model="form.accent"
          :label="t('pages.settings.accent')"
          :options="accentOptions"
        />
      </SettingsSection>

      <SettingsSection
        id="page-languages"
        :title="t('pages.settings.languages')"
        :description="t('pages.settings.languagesHint')"
      >
        <SelectField
          id="page-default-locale"
          v-model="form.defaultLocale"
          :label="t('pages.settings.defaultLocale')"
          :options="localeOptions"
        />
        <CheckboxGroup
          v-model="enabledLocales"
          :legend="t('pages.settings.locales')"
          :options="enabledOptions"
        />
        <TimezoneField
          id="page-timezone"
          v-model="form.timezone"
          :label="t('pages.settings.timezone')"
          :placeholder="t('pages.settings.timezonePlaceholder')"
          :error="errorFor('timezone')"
        />
      </SettingsSection>

      <SettingsSection
        id="page-auto-incidents"
        :title="t('pages.settings.autoIncidents')"
        :description="t('pages.settings.autoIncidentsHint')"
      >
        <ChoiceGroup
          v-model="form.autoIncidents"
          :label="t('pages.settings.autoIncidents')"
          :options="modeOptions"
          class="w-fit max-w-full"
        />
        <p class="text-sm text-muted">{{ t(`pages.settings.autoModeHints.${form.autoIncidents}`) }}</p>
        <FormField
          v-if="form.autoIncidents === 'auto_publish'"
          id="page-grace"
          v-model="form.graceMinutes"
          inputmode="numeric"
          :label="t('pages.settings.grace')"
          :description="t('pages.settings.graceHint')"
          :suffix="t('units.minutes')"
          :error="errorFor('grace_minutes')"
        />
      </SettingsSection>
    </SettingsPage>

    <div class="mt-8 grid gap-4 border-t border-divider pt-6">
      <CommandAlert
        :error="save.error.value"
        :shown="shownFields"
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
          :loading="save.isPending.value"
        >
          {{ t('pages.settings.save') }}
        </Button>
      </div>
    </div>
  </form>
  <SettingsPage
    v-if="manage && page"
    class="mt-10 border-t border-divider pt-8"
  >
    <SettingsSection
      id="page-delete"
      :title="t('pages.settings.deleteTitle')"
      :description="t('pages.settings.deleteHint')"
      destructive
    >
      <Alert
        v-if="remove.error.value"
        color="destructive"
      >
        <p class="text-sm">{{ message(remove.error.value) }}</p>
      </Alert>
      <div>
        <Button
          type="button"
          variant="destructive"
          :loading="remove.isPending.value"
          @click="askDelete"
        >
          <Icon name="lucide:trash-2" />
          {{ t('pages.settings.delete') }}
        </Button>
      </div>
    </SettingsSection>
  </SettingsPage>
</template>
