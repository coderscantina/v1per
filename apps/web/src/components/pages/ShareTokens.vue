<script setup lang="ts">
import { useMutation } from '@tanstack/vue-query'

import { pagesResource } from '~/api/resources/pages'
import SecretReveal from '~/components/account/SecretReveal.vue'
import CommandAlert from '~/components/CommandAlert.vue'
import FormField from '~/components/FormField.vue'
import Icon from '~/components/Icon.vue'
import { Alert } from '~/components/ui/alert'
import { Button } from '~/components/ui/button'
import { Panel } from '~/components/ui/panel'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '~/components/ui/table'
import { useConfirm } from '~/composables/useConfirm'
import { useDateFormat } from '~/composables/useDateFormat'
import { useErrorMessage } from '~/composables/useErrorMessage'
import type { Page, PageShareToken, ShareTokenCreated } from '~/generated'
import { NAME_MAX_CHARS } from '~/generated/limits'
import { pageUrl } from '~/lib/pages'
import { useI18n } from '~/plugins/i18n'

/**
 * Share links of a restricted page: each opens the page without signing in until it expires or
 * is revoked. The link carries a secret the server shows once, right after creating it.
 */
const props = defineProps<{ page: Page; tokens: PageShareToken[]; restricted: boolean }>()

const { t } = useI18n()
const { fieldMessage, message } = useErrorMessage()
const { confirm } = useConfirm()
const { date } = useDateFormat()

const name = ref('')
const expires = ref('')
const created = shallowRef<ShareTokenCreated | null>(null)
const link = computed(() => (created.value?.secret ? `${pageUrl(props.page)}/t/${created.value.secret}` : null))

/** A date picked for expiry means through that day, in the reader's timezone. */
const expiresAt = (day: string): number | undefined => {
  if (day === '') return undefined
  const [year = 0, month = 1, date = 1] = day.split('-').map(Number)
  return new Date(year, month - 1, date + 1).getTime() - 1
}

const create = useMutation({
  mutationFn: () => pagesResource.createShareToken({ page: props.page.id, name: name.value.trim(), expires_at: expiresAt(expires.value) }),
  onSuccess: (answer) => {
    created.value = answer
    name.value = ''
    expires.value = ''
  },
})

const revoke = useMutation({ mutationFn: (token: PageShareToken) => pagesResource.revokeShareToken(token.id) })
const askRevoke = async (token: PageShareToken) => {
  const ok = await confirm({
    title: t('pages.tokens.revokeTitle'),
    message: t('pages.tokens.revokeConfirm', { name: token.name }),
    confirmLabel: t('pages.tokens.revoke'),
    variant: 'destructive',
  })
  if (ok) revoke.mutate(token)
}
const expired = (token: PageShareToken) => token.expires_at !== undefined && token.expires_at < Date.now()
</script>

<template>
  <Panel
    id="page-share-links"
    :title="t('pages.tokens.title')"
    :meta="String(tokens.length)"
  >
    <div class="grid gap-4">
      <p class="text-sm text-muted">{{ restricted ? t('pages.tokens.intro') : t('pages.tokens.introPublic') }}</p>

      <div
        v-if="created"
        class="grid gap-3"
        data-testid="share-link"
      >
        <SecretReveal
          id="page-share-link"
          :label="t('pages.tokens.link')"
          :secret="link"
        />
        <div>
          <Button
            type="button"
            variant="fill"
            size="sm"
            @click="created = null"
          >
            {{ t('actions.done') }}
          </Button>
        </div>
      </div>

      <form
        v-else
        class="grid gap-3 sm:grid-cols-[minmax(0,1fr)_12rem_auto] sm:items-start"
        novalidate
        @submit.prevent="create.mutate()"
      >
        <FormField
          id="share-token-name"
          v-model="name"
          autocomplete="off"
          :label="t('pages.tokens.name')"
          :description="t('pages.tokens.nameHint', { max: NAME_MAX_CHARS })"
          :error="fieldMessage(create.error.value, 'name')"
          required
        />
        <FormField
          id="share-token-expires"
          v-model="expires"
          type="date"
          :label="t('pages.tokens.expires')"
          :description="t('pages.tokens.expiresHint')"
          :error="fieldMessage(create.error.value, 'expires_at')"
        />
        <Button
          type="submit"
          variant="fill"
          class="sm:mt-6"
          :loading="create.isPending.value"
          :disabled="name.trim() === ''"
        >
          <Icon name="lucide:link" />
          {{ t('pages.tokens.create') }}
        </Button>
        <CommandAlert
          class="sm:col-span-3"
          :error="create.error.value"
          :shown="['name', 'expires_at']"
        />
      </form>

      <Alert
        v-if="revoke.error.value"
        color="destructive"
      >
        <p class="text-sm">{{ message(revoke.error.value) }}</p>
      </Alert>

      <Table
        v-if="tokens.length > 0"
        :label="t('pages.tokens.title')"
      >
        <TableHeader>
          <TableRow>
            <TableHead>{{ t('pages.tokens.name') }}</TableHead>
            <TableHead>{{ t('pages.tokens.expires') }}</TableHead>
            <TableHead class="max-sm:hidden">{{ t('pages.tokens.created') }}</TableHead>
            <TableHead><span class="sr-only">{{ t('actions.label') }}</span></TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          <TableRow
            v-for="token in tokens"
            :key="token.id"
            :data-token="token.name"
          >
            <TableCell
              kind="subject"
              class="max-w-0 min-w-32"
            >
              <span class="block truncate">{{ token.name }}</span>
            </TableCell>
            <TableCell
              kind="secondary"
              class="whitespace-nowrap"
            >
              <template v-if="token.expires_at === undefined">{{ t('pages.tokens.never') }}</template>
              <span
                v-else
                :class="expired(token) ? 'text-destructive' : undefined"
              >
                {{ expired(token) ? t('pages.tokens.expiredOn', { date: date(token.expires_at) }) : date(token.expires_at) }}
              </span>
            </TableCell>
            <TableCell
              kind="secondary"
              class="max-sm:hidden"
            >
              {{ token.created_by ? t('pages.tokens.createdBy', { date: date(token.created_at), name: token.created_by }) : date(token.created_at) }}
            </TableCell>
            <TableCell class="text-right">
              <Button
                type="button"
                variant="ghost"
                size="icon"
                :disabled="revoke.isPending.value"
                :aria-label="t('pages.tokens.revokeNamed', { name: token.name })"
                @click="askRevoke(token)"
              >
                <Icon name="lucide:trash-2" />
              </Button>
            </TableCell>
          </TableRow>
        </TableBody>
      </Table>
    </div>
  </Panel>
</template>
