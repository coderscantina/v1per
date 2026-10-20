<script setup lang="ts">
// Adapted from holocron (84fc54ec): apps/web/src/components/account/TwoFactorSection.vue.
// Sensitive steps go through the reauthentication dialog.
import { queryKeys } from '~/api/query-keys'
import { accountResource } from '~/api/resources/account'
import RecoveryCodes from '~/components/account/RecoveryCodes.vue'
import SettingsSection from '~/components/account/SettingsSection.vue'
import TwoFactorSetup from '~/components/account/TwoFactorSetup.vue'
import Icon from '~/components/Icon.vue'
import { Alert } from '~/components/ui/alert'
import { Badge } from '~/components/ui/badge'
import { Button } from '~/components/ui/button'
import { Dialog, DialogContent, DialogHeaderCombined } from '~/components/ui/dialog'
import { Skeleton } from '~/components/ui/skeleton'
import { useTwoFactorStatus } from '~/composables/useAccount'
import { useConfirm } from '~/composables/useConfirm'
import { useDateFormat } from '~/composables/useDateFormat'
import { useErrorMessage } from '~/composables/useErrorMessage'
import { useReauth } from '~/composables/useReauth'
import { useI18n } from '~/plugins/i18n'
import { queryClient } from '~/plugins/vue-query'

const { t } = useI18n()
const { message } = useErrorMessage()
const { confirm } = useConfirm()
const { date } = useDateFormat()
const reauth = useReauth()
const status = useTwoFactorStatus()

const setupOpen = ref(false)
/** Fixed when the dialog opens, so confirming does not retitle it mid-flow. */
const replacing = ref(false)
const openSetup = () => {
  replacing.value = enrolled.value
  setupOpen.value = true
}
const codesOpen = ref(false)
const codes = ref<string[] | null>(null)
const busy = ref(false)
const error = ref<unknown>(null)
const notice = ref<string | null>(null)

const enrolled = computed(() => status.data.value?.enrolled ?? false)
const required = computed(() => status.data.value?.required ?? false)

const refresh = () => queryClient.invalidateQueries({ queryKey: queryKeys.twoFactor })

const act = async (action: () => Promise<void>) => {
  busy.value = true
  error.value = null
  notice.value = null
  try {
    await action()
  } catch (failure) {
    error.value = failure
  } finally {
    busy.value = false
  }
}

const regenerate = async () => {
  const ok = await confirm({
    title: t('twoFactor.regenerate'),
    message: t('twoFactor.regenerateConfirm'),
    confirmLabel: t('twoFactor.regenerate'),
  })
  if (!ok) return
  await act(async () => {
    const result = await reauth.run('twoFactor.regenerateRecoveryCodes', {}, () => accountResource.regenerateRecoveryCodes())
    if (!result) return
    codes.value = result.secret
    codesOpen.value = true
    await refresh()
  })
}

const disable = async () => {
  const ok = await confirm({
    title: t('twoFactor.disable'),
    message: t('twoFactor.disableConfirm'),
    confirmLabel: t('twoFactor.disable'),
    variant: 'destructive',
  })
  if (!ok) return
  await act(async () => {
    const result = await reauth.run('twoFactor.disable', {}, () => accountResource.disableTwoFactor())
    if (!result) return
    queryClient.setQueryData(queryKeys.twoFactor, result)
    notice.value = t('twoFactor.disabled')
  })
}

const onEnrolled = () => {
  void refresh()
  notice.value = t('twoFactor.enabled')
}
</script>

<template>
  <SettingsSection
    id="two-factor"
    :title="t('twoFactor.title')"
    :description="t('twoFactor.description')"
  >
    <Skeleton
      v-if="status.isPending.value"
      class="h-20 w-full rounded-panel"
    />
    <Alert
      v-else-if="status.error.value"
      color="destructive"
    >
      <p class="text-sm">{{ message(status.error.value) }}</p>
    </Alert>
    <div
      v-else
      class="flex flex-wrap items-center justify-between gap-3 rounded-panel border border-hairline bg-card p-4"
    >
      <div class="flex min-w-0 items-center gap-3">
        <span
          class="grid size-9 shrink-0 place-items-center rounded-lg"
          :class="enrolled ? 'bg-success-background/15 text-success' : 'bg-fill text-muted'"
        >
          <Icon
            :name="enrolled ? 'lucide:shield-check' : 'lucide:shield'"
            size="18"
            aria-hidden="true"
          />
        </span>
        <div class="grid min-w-0 gap-0.5">
          <div class="flex flex-wrap items-center gap-2 text-sm font-medium text-primary">
            {{ t('twoFactor.authenticator') }}
            <Badge
              :variant="enrolled ? 'success' : 'secondary'"
              size="xs"
            >
              {{ enrolled ? t('twoFactor.on') : t('twoFactor.off') }}
            </Badge>
            <Badge
              v-if="required"
              variant="warning"
              size="xs"
            >
              {{ t('twoFactor.required') }}
            </Badge>
          </div>
          <p class="text-sm text-pretty text-muted">
            {{
              enrolled
                ? t('twoFactor.onSince', {
                  date: status.data.value?.enrolled_at ? date(status.data.value.enrolled_at) : '',
                  left: status.data.value?.recovery_codes_left ?? 0,
                })
                : t('twoFactor.offHint')
            }}
          </p>
        </div>
      </div>
      <div class="flex flex-wrap gap-2">
        <Button
          v-if="!enrolled"
          variant="primary"
          @click="openSetup"
        >
          {{ t('twoFactor.enable') }}
        </Button>
        <template v-else>
          <Button
            size="sm"
            variant="outline"
            :disabled="busy"
            @click="regenerate"
          >
            {{ t('twoFactor.regenerate') }}
          </Button>
          <Button
            size="sm"
            variant="outline"
            @click="openSetup"
          >
            {{ t('twoFactor.reenroll') }}
          </Button>
          <Button
            v-if="!required"
            variant="destructive"
            size="sm"
            :disabled="busy"
            @click="disable"
          >
            {{ t('twoFactor.disable') }}
          </Button>
        </template>
      </div>
    </div>
    <p
      v-if="enrolled && required"
      class="text-sm text-muted"
    >
      {{ t('twoFactor.requiredHint') }}
    </p>
    <div
      aria-live="polite"
      class="empty:hidden"
    >
      <Alert
        v-if="notice"
        color="success"
      >
        <p class="text-sm">{{ notice }}</p>
      </Alert>
      <Alert
        v-else-if="error"
        color="destructive"
      >
        <p class="text-sm">{{ message(error) }}</p>
      </Alert>
    </div>

    <Dialog v-model:open="setupOpen">
      <DialogContent
        class="max-w-md"
        @interact-outside="(event) => event.preventDefault()"
      >
        <DialogHeaderCombined
          :title="replacing ? t('twoFactor.reenroll') : t('twoFactor.enroll.title')"
          :description="t('twoFactor.setup.description')"
        />
        <TwoFactorSetup
          v-if="setupOpen"
          cancellable
          @enrolled="onEnrolled"
          @done="setupOpen = false"
          @cancel="setupOpen = false"
        />
      </DialogContent>
    </Dialog>

    <Dialog v-model:open="codesOpen">
      <DialogContent class="max-w-md">
        <DialogHeaderCombined
          :title="t('twoFactor.regenerate')"
          :description="t('twoFactor.regeneratedDescription')"
        />
        <RecoveryCodes :codes="codes" />
      </DialogContent>
    </Dialog>
  </SettingsSection>
</template>
