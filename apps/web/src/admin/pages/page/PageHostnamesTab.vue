<script setup lang="ts">
import { useMutation } from '@tanstack/vue-query'

import { pagesResource } from '~/api/resources/pages'
import CommandAlert from '~/components/CommandAlert.vue'
import FormField from '~/components/FormField.vue'
import Icon from '~/components/Icon.vue'
import { Alert } from '~/components/ui/alert'
import { Button } from '~/components/ui/button'
import { Panel } from '~/components/ui/panel'
import { useConfirm } from '~/composables/useConfirm'
import { useErrorMessage } from '~/composables/useErrorMessage'
import { usePageEditor } from '~/composables/usePages'
import { PAGE_HOSTNAMES_MAX } from '~/generated/limits'
import { useI18n } from '~/plugins/i18n'

/**
 * Custom domains: a claimed hostname serves the page at its root. The server routes by the
 * `Host` header; DNS, the proxy and its certificate are set up outside, as the runbook says.
 */
const { t } = useI18n()
const { fieldMessage, message } = useErrorMessage()
const { confirm } = useConfirm()
const editor = usePageEditor()
const page = computed(() => editor.data.value?.page ?? null)
const instanceHost = window.location.host

const hostname = ref('')
const claim = useMutation({
  mutationFn: () => pagesResource.claimHostname(page.value?.id ?? '', hostname.value.trim().toLowerCase()),
  onSuccess: () => (hostname.value = ''),
})
const release = useMutation({ mutationFn: (name: string) => pagesResource.releaseHostname(page.value?.id ?? '', name) })
const askRelease = async (name: string) => {
  const ok = await confirm({
    title: t('pages.hostnames.releaseTitle'),
    message: t('pages.hostnames.releaseConfirm', { hostname: name }),
    confirmLabel: t('pages.hostnames.release'),
    variant: 'destructive',
  })
  if (ok) release.mutate(name)
}
</script>

<template>
  <div
    v-if="page"
    class="grid gap-6"
  >
    <p class="text-sm text-muted">{{ t('pages.hostnames.intro') }}</p>

    <Panel
      id="page-hostnames"
      :title="t('pages.hostnames.title')"
      :meta="`${page.hostnames.length} / ${PAGE_HOSTNAMES_MAX}`"
      flush
    >
      <ul
        v-if="page.hostnames.length > 0"
        class="divide-y divide-divider border-t border-hairline"
      >
        <li
          v-for="name in page.hostnames"
          :key="name"
          class="flex min-h-12 items-center gap-3 px-4.5 py-2"
          :data-hostname="name"
        >
          <Icon
            name="lucide:globe"
            size="16"
            class="shrink-0 text-muted"
            aria-hidden="true"
          />
          <a
            :href="`https://${name}`"
            target="_blank"
            rel="noopener"
            class="min-w-0 flex-1 truncate rounded-sm font-medium text-primary underline-offset-4 hover:underline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring"
          >{{ name }}</a>
          <Button
            v-if="editor.manage.value"
            type="button"
            variant="ghost"
            size="sm"
            :disabled="release.isPending.value"
            :aria-label="t('pages.hostnames.releaseNamed', { hostname: name })"
            @click="askRelease(name)"
          >
            {{ t('pages.hostnames.release') }}
          </Button>
        </li>
      </ul>
      <p
        v-else
        class="border-t border-hairline px-4.5 py-8 text-center text-sm text-muted"
      >
        {{ t('pages.hostnames.none') }}
      </p>
      <Alert
        v-if="release.error.value"
        color="destructive"
        class="m-3 w-auto"
      >
        <p class="text-sm">{{ message(release.error.value) }}</p>
      </Alert>
    </Panel>

    <form
      v-if="editor.manage.value"
      class="grid gap-3"
      novalidate
      @submit.prevent="claim.mutate()"
    >
      <div class="grid gap-3 sm:grid-cols-[minmax(0,1fr)_auto] sm:items-start">
        <FormField
          id="page-hostname"
          v-model="hostname"
          autocomplete="off"
          inputmode="url"
          placeholder="status.example.com"
          :label="t('pages.hostnames.add')"
          :description="t('pages.hostnames.addHint')"
          :error="fieldMessage(claim.error.value, 'hostname')"
        />
        <Button
          type="submit"
          variant="fill"
          class="sm:mt-6"
          :loading="claim.isPending.value"
          :disabled="hostname.trim() === '' || page.hostnames.length >= PAGE_HOSTNAMES_MAX"
        >
          {{ t('pages.hostnames.claim') }}
        </Button>
      </div>
      <CommandAlert
        :error="claim.error.value"
        :shown="['hostname']"
      />
    </form>

    <Panel
      id="page-hostname-setup"
      :title="t('pages.hostnames.setupTitle')"
    >
      <ol class="grid list-decimal gap-2 pl-5 text-sm text-foreground">
        <li>{{ t('pages.hostnames.stepClaim') }}</li>
        <li>{{ t('pages.hostnames.stepDns', { host: instanceHost }) }}</li>
        <li>{{ t('pages.hostnames.stepProxy') }}</li>
      </ol>
      <p class="mt-3 text-sm text-muted">{{ t('pages.hostnames.grantNote') }}</p>
      <p class="mt-3 text-sm text-muted">{{ t('pages.hostnames.runbook') }}</p>
    </Panel>
  </div>
</template>
