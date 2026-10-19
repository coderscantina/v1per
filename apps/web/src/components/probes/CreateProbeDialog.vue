<script setup lang="ts">
import { useMutation } from '@tanstack/vue-query'

import { probesResource } from '~/api/resources/probes'
import SecretReveal from '~/components/account/SecretReveal.vue'
import CommandAlert from '~/components/CommandAlert.vue'
import CopyButton from '~/components/CopyButton.vue'
import FormField from '~/components/FormField.vue'
import { Button } from '~/components/ui/button'
import { Dialog, DialogContent, DialogHeaderCombined } from '~/components/ui/dialog'
import { Label } from '~/components/ui/label'
import { Textarea } from '~/components/ui/textarea'
import { useErrorMessage } from '~/composables/useErrorMessage'
import { usePublicOrigin } from '~/composables/useMonitors'
import type { NodeId, ProbeCreated, ProbesCreateArgs } from '~/generated'
import { NAME_MAX_CHARS } from '~/generated/limits'
import { probeCommand } from '~/lib/probes'
import { useI18n } from '~/plugins/i18n'

/**
 * Registers a probe on a node in two steps: name it, then copy the command that starts its
 * agent. The second step is the only time the token exists outside its hash, so the dialog does
 * not close on an outside click then. The list updates from the node's topic.
 */
const props = defineProps<{ node: NodeId }>()
const open = defineModel<boolean>('open', { default: false })

const { t } = useI18n()
const { fieldMessage } = useErrorMessage()
const origin = usePublicOrigin()
const name = ref('')
const created = ref<ProbeCreated | null>(null)

const create = useMutation({
  mutationFn: (args: ProbesCreateArgs) => probesResource.create(args),
  onSuccess: (result) => (created.value = result),
})

watch(open, (value) => {
  if (value) return
  name.value = ''
  created.value = null
  create.reset()
})

const command = computed(() => (created.value?.secret ? probeCommand(origin.value, created.value.secret) : null))
const submit = () => create.mutate({ node: props.node, name: name.value })

const select = (event: FocusEvent) => {
  if (event.target instanceof HTMLTextAreaElement) event.target.select()
}
</script>

<template>
  <Dialog v-model:open="open">
    <DialogContent
      class="max-w-2xl"
      @interact-outside="(event) => created && event.preventDefault()"
    >
      <template v-if="created">
        <DialogHeaderCombined
          :title="t('probes.createdTitle')"
          :description="t('probes.createdDescription')"
        />
        <SecretReveal
          id="probe-token"
          :label="t('probes.token')"
          :secret="created.secret"
        />
        <div
          v-if="command"
          class="grid gap-1.5"
        >
          <Label for="probe-command">{{ t('probes.command') }}</Label>
          <div class="flex min-w-0 gap-2 max-sm:flex-col">
            <Textarea
              id="probe-command"
              :model-value="command"
              readonly
              rows="2"
              class="min-h-0 min-w-0 resize-none font-mono text-xs break-all max-sm:min-h-24"
              aria-describedby="probe-command-hint"
              @focus="select"
            />
            <CopyButton :text="command" />
          </div>
          <p
            id="probe-command-hint"
            class="text-xs text-muted"
          >
            {{ t('probes.commandHint') }}
          </p>
        </div>
        <div class="flex justify-end">
          <Button
            variant="primary"
            @click="open = false"
          >
            {{ t('actions.done') }}
          </Button>
        </div>
      </template>
      <template v-else>
        <DialogHeaderCombined
          :title="t('probes.create')"
          :description="t('probes.createDescription')"
        />
        <form
          class="grid gap-4"
          novalidate
          @submit.prevent="submit"
        >
          <FormField
            id="probe-name"
            v-model="name"
            autocomplete="off"
            :label="t('probes.name')"
            :description="t('probes.nameHint', { max: NAME_MAX_CHARS })"
            :error="fieldMessage(create.error.value, 'name')"
            required
          />
          <CommandAlert
            :error="create.error.value"
            :shown="['name']"
          />
          <div class="flex justify-end gap-2">
            <Button
              type="button"
              variant="ghost"
              @click="open = false"
            >
              {{ t('actions.cancel') }}
            </Button>
            <Button
              type="submit"
              variant="primary"
              :loading="create.isPending.value"
              :disabled="name.trim() === ''"
            >
              {{ t('probes.create') }}
            </Button>
          </div>
        </form>
      </template>
    </DialogContent>
  </Dialog>
</template>
