<script setup lang="ts">
// Adapted from holocron (84fc54ec): apps/web/src/components/account/RecoveryCodes.vue. A
// replayed answer withholds the codes, which says so instead of an empty list.
import CopyButton from '~/components/CopyButton.vue'
import Icon from '~/components/Icon.vue'
import { Alert } from '~/components/ui/alert'
import { Button } from '~/components/ui/button'
import { useI18n } from '~/plugins/i18n'

/** Recovery codes are shown once. Copy and download are the two ways to keep them. */
const props = defineProps<{ codes: string[] | null }>()

const { t } = useI18n()

const asText = computed(() => (props.codes ?? []).join('\n'))

const download = () => {
  const url = URL.createObjectURL(new Blob([`${asText.value}\n`], { type: 'text/plain' }))
  const link = document.createElement('a')
  link.href = url
  link.download = 'v1per-recovery-codes.txt'
  link.click()
  URL.revokeObjectURL(url)
}
</script>

<template>
  <div
    v-if="codes"
    class="grid gap-3"
  >
    <Alert color="warning">
      <p class="text-sm">{{ t('twoFactor.codesOnce') }}</p>
    </Alert>
    <ul
      class="grid grid-cols-2 gap-2"
      :aria-label="t('twoFactor.recoveryCodes')"
    >
      <li
        v-for="code in codes"
        :key="code"
        class="rounded-md border border-hairline px-2 py-1 text-center font-mono text-sm text-primary"
      >
        {{ code }}
      </li>
    </ul>
    <div class="grid grid-cols-2 gap-2">
      <CopyButton :text="asText" />
      <Button
        type="button"
        variant="outline"
        @click="download"
      >
        <Icon name="lucide:download" />
        {{ t('twoFactor.download') }}
      </Button>
    </div>
  </div>
  <Alert
    v-else
    color="info"
  >
    <p class="text-sm">{{ t('twoFactor.codesWithheld') }}</p>
  </Alert>
</template>
