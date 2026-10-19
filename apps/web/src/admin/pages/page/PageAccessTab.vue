<script setup lang="ts">
import { useMutation } from '@tanstack/vue-query'

import { pagesResource } from '~/api/resources/pages'
import SettingsPage from '~/components/account/SettingsPage.vue'
import SettingsSection from '~/components/account/SettingsSection.vue'
import CommandAlert from '~/components/CommandAlert.vue'
import FormField from '~/components/FormField.vue'
import ShareTokens from '~/components/pages/ShareTokens.vue'
import SwitchField from '~/components/SwitchField.vue'
import TextareaField from '~/components/TextareaField.vue'
import { Alert } from '~/components/ui/alert'
import { Badge } from '~/components/ui/badge'
import { Button } from '~/components/ui/button'
import { useErrorMessage } from '~/composables/useErrorMessage'
import { usePageEditor } from '~/composables/usePages'
import type { PageDetail } from '~/generated'
import { PAGE_ALLOWED_EMAILS_MAX } from '~/generated/limits'
import { accessFromPage, accessModeLabels, accessModes, toAccessArgs, validateAccess, type AccessForm } from '~/lib/pages'
import { useI18n } from '~/plugins/i18n'

/**
 * Who may open the page besides the node's team, who always may after signing in: anyone when
 * public, else any mix of a password, sign-in links to allowed addresses, and share links. A
 * restricted page with none of them is for the team only.
 */
const { t } = useI18n()
const { fieldMessage, fieldProblem } = useErrorMessage()
const editor = usePageEditor()
const detail = computed(() => editor.data.value ?? null)
const page = computed(() => detail.value?.page ?? null)
const modes = computed(() => (detail.value ? accessModes(detail.value.page.access, detail.value.share_tokens.length) : []))
const passwordStored = computed(() => page.value?.access.password?.set ?? false)

const form = ref<AccessForm | null>(null)
const basedOn = ref<number | null>(null)
const load = (from: PageDetail) => {
  form.value = accessFromPage(from.page.access)
  basedOn.value = from.page.revision
}
watch(
  detail,
  (current) => {
    if (current && basedOn.value === null) load(current)
  },
  { immediate: true }
)
// Share links change the revision too; a form nobody touched follows them.
const pristine = computed(() => detail.value !== null && form.value !== null && JSON.stringify(form.value) === JSON.stringify(accessFromPage(detail.value.page.access)))
watch(
  () => detail.value?.page.revision,
  () => {
    if (detail.value && pristine.value) load(detail.value)
  }
)

const submitted = ref(false)
const clientErrors = computed(() => (submitted.value && form.value ? validateAccess(form.value) : {}))
const save = useMutation({
  mutationFn: ({ current, revision }: { current: AccessForm; revision: number }) => {
    if (!page.value) throw new Error('page not loaded')
    return pagesResource.updateAccess(toAccessArgs(current, page.value, revision))
  },
  onSuccess: (answer) => {
    load(answer)
    submitted.value = false
  },
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
  if (Object.keys(validateAccess(current)).length > 0) {
    void nextTick(() => document.querySelector<HTMLElement>('[aria-invalid="true"]')?.focus())
    return
  }
  save.mutate({ current, revision: basedOn.value }, { onSuccess: () => (saved.value = true) })
}
const reload = () => {
  if (detail.value) load(detail.value)
  save.reset()
}
const setPassword = (action: AccessForm['passwordAction']) => {
  if (!form.value) return
  form.value.passwordAction = action
  form.value.password = ''
  if (action === 'set') void nextTick(() => document.getElementById('page-password')?.focus())
}
</script>

<template>
  <div
    v-if="page && detail"
    class="grid gap-8"
  >
    <div class="grid gap-3">
      <div
        class="flex flex-wrap items-center gap-2"
        data-testid="access-modes"
      >
        <span class="text-sm text-muted">{{ t('pages.access.now') }}</span>
        <Badge
          v-for="mode in modes"
          :key="mode"
          :variant="mode === 'public' ? 'success' : 'surface'"
          size="sm"
          :data-mode="mode"
        >
          {{ t(accessModeLabels[mode]) }}
        </Badge>
      </div>
      <Alert icon="lucide:users">
        <p class="text-sm">{{ t('pages.access.grantLogin') }}</p>
      </Alert>
    </div>

    <Alert
      v-if="!editor.manage.value"
      icon="lucide:info"
    >
      <p class="text-sm">{{ t('pages.readOnly') }}</p>
    </Alert>

    <template v-else-if="form">
      <form
        novalidate
        @submit.prevent="submit"
      >
        <SettingsPage>
          <SettingsSection
            id="page-public"
            :title="t('pages.access.publicTitle')"
          >
            <SwitchField
              id="page-public-switch"
              v-model="form.public"
              :label="t('pages.access.public')"
              :description="t('pages.access.publicHint')"
            />
          </SettingsSection>

          <SettingsSection
            id="page-password-section"
            :title="t('pages.access.password')"
            :description="form.public ? t('pages.access.whenRestricted') : t('pages.access.passwordHint')"
          >
            <template v-if="form.passwordAction === 'set'">
              <FormField
                id="page-password"
                v-model="form.password"
                type="password"
                autocomplete="new-password"
                :label="passwordStored ? t('pages.access.newPassword') : t('pages.access.password')"
                :error="errorFor('password')"
                required
              />
              <div>
                <Button
                  type="button"
                  variant="ghost"
                  size="sm"
                  @click="setPassword('keep')"
                >
                  {{ passwordStored ? t('pages.access.keepPassword') : t('actions.cancel') }}
                </Button>
              </div>
            </template>
            <template v-else>
              <p
                class="text-sm text-primary"
                data-testid="password-state"
              >
                {{
                  form.passwordAction === 'clear'
                    ? t('pages.access.passwordRemoved')
                    : passwordStored
                      ? t('pages.access.passwordSet')
                      : t('pages.access.passwordNone')
                }}
              </p>
              <div class="flex flex-wrap gap-2">
                <Button
                  v-if="form.passwordAction === 'clear'"
                  type="button"
                  variant="fill"
                  size="sm"
                  @click="setPassword('keep')"
                >
                  {{ t('pages.access.keepPassword') }}
                </Button>
                <template v-else>
                  <Button
                    type="button"
                    variant="fill"
                    size="sm"
                    @click="setPassword('set')"
                  >
                    {{ passwordStored ? t('pages.access.changePassword') : t('pages.access.setPassword') }}
                  </Button>
                  <Button
                    v-if="passwordStored"
                    type="button"
                    variant="ghost"
                    size="sm"
                    @click="setPassword('clear')"
                  >
                    {{ t('pages.access.removePassword') }}
                  </Button>
                </template>
              </div>
            </template>
          </SettingsSection>

          <SettingsSection
            id="page-email"
            :title="t('pages.access.email')"
            :description="form.public ? t('pages.access.whenRestricted') : t('pages.access.emailHint')"
          >
            <Alert
              v-if="!detail.smtp_configured"
              color="warning"
            >
              <p class="text-sm">{{ t('pages.access.noSmtp') }}</p>
            </Alert>
            <TextareaField
              id="page-emails"
              v-model="form.emails"
              :rows="3"
              :label="t('pages.access.emails')"
              :description="t('pages.access.emailsHint', { max: PAGE_ALLOWED_EMAILS_MAX })"
              placeholder="ops@example.com"
              :error="errorFor('emails')"
            />
            <TextareaField
              id="page-domains"
              v-model="form.domains"
              :rows="2"
              :label="t('pages.access.domains')"
              :description="t('pages.access.domainsHint')"
              placeholder="example.com"
              :error="errorFor('domains')"
            />
          </SettingsSection>
        </SettingsPage>

        <div class="mt-8 grid gap-4 border-t border-divider pt-6">
          <CommandAlert
            :error="save.error.value"
            :shown="['password', 'emails', 'domains']"
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
              {{ t('pages.access.save') }}
            </Button>
          </div>
        </div>
      </form>

      <ShareTokens
        :page="page"
        :tokens="detail.share_tokens"
        :restricted="!page.access.public"
      />
    </template>
  </div>
</template>
