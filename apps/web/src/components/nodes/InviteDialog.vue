<script setup lang="ts">
// Adapted from holocron (84fc54ec): apps/web/src/components/workspace/InviteDialog.vue. The
// invitation is for one email address; without an email service yet the dialog shows the
// link once for the inviter to pass on.
import { useMutation } from '@tanstack/vue-query'

import { membersResource } from '~/api/resources/members'
import SecretReveal from '~/components/account/SecretReveal.vue'
import CommandAlert from '~/components/CommandAlert.vue'
import FormField from '~/components/FormField.vue'
import { Button } from '~/components/ui/button'
import { Dialog, DialogContent, DialogHeaderCombined } from '~/components/ui/dialog'
import { Label } from '~/components/ui/label'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '~/components/ui/select'
import { useErrorMessage } from '~/composables/useErrorMessage'
import type { InvitationCreated, MembersInviteArgs, Node, Role } from '~/generated'
import { INVITATION_LIFETIME_DEFAULT_DAYS, INVITATION_LIFETIME_MAX_DAYS } from '~/generated/limits'
import { assignableRoles } from '~/lib/access'
import { roleLabels } from '~/lib/roles'
import { useI18n } from '~/plugins/i18n'

const props = defineProps<{ node: Node }>()
const open = defineModel<boolean>('open', { default: false })

const { t } = useI18n()
const { fieldMessage } = useErrorMessage()

const roles = computed(() => assignableRoles(props.node.role))
const fallbackRole = (): Role => (roles.value.includes('member') ? 'member' : (roles.value.at(-1) ?? 'viewer'))

const email = ref('')
const role = ref<Role>(fallbackRole())
const days = ref(String(INVITATION_LIFETIME_DEFAULT_DAYS))
const created = ref<InvitationCreated | null>(null)

const invite = useMutation({
  mutationFn: (args: MembersInviteArgs) => membersResource.invite(args),
  onSuccess: (result) => {
    created.value = result
  },
})

watch(open, (value) => {
  if (value) return
  email.value = ''
  role.value = fallbackRole()
  days.value = String(INVITATION_LIFETIME_DEFAULT_DAYS)
  created.value = null
  invite.reset()
})

/** Without a public URL the server sends the path alone; this app's origin completes it. */
const link = computed(() => {
  const secret = created.value?.secret
  if (!secret) return null
  return secret.link.startsWith('/') ? `${window.location.origin}${secret.link}` : secret.link
})
const lifetime = computed(() => Number.parseInt(days.value, 10))
const valid = computed(
  () =>
    email.value.trim() !== '' &&
    Number.isInteger(lifetime.value) &&
    lifetime.value >= 1 &&
    lifetime.value <= INVITATION_LIFETIME_MAX_DAYS
)

const onRole = (value: unknown) => {
  const next = roles.value.find((entry) => entry === value)
  if (next) role.value = next
}

const submit = () =>
  invite.mutate({ node: props.node.id, email: email.value.trim(), role: role.value, lifetime_days: lifetime.value })
</script>

<template>
  <Dialog v-model:open="open">
    <DialogContent
      class="max-w-md"
      @interact-outside="(event) => created && event.preventDefault()"
    >
      <template v-if="created">
        <DialogHeaderCombined
          :title="t('invitations.createdTitle')"
          :description="t('invitations.createdDescription', { email: created.invitation.email })"
        />
        <SecretReveal
          id="invitation-link"
          :label="t('invitations.link')"
          :secret="link"
        />
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
          :title="t('invitations.create')"
          :description="t('invitations.createDescription', { node: node.name })"
        />
        <form
          class="grid gap-4"
          novalidate
          @submit.prevent="submit"
        >
          <FormField
            id="invite-email"
            v-model="email"
            type="email"
            autocomplete="off"
            :label="t('invitations.email')"
            :error="fieldMessage(invite.error.value, 'email')"
            required
          />
          <div class="grid gap-1.5">
            <Label for="invite-role">{{ t('members.role') }}</Label>
            <Select
              :model-value="role"
              @update:model-value="onRole"
            >
              <SelectTrigger id="invite-role">
                <SelectValue>{{ t(roleLabels[role]) }}</SelectValue>
              </SelectTrigger>
              <SelectContent>
                <SelectItem
                  v-for="entry in roles"
                  :key="entry"
                  :value="entry"
                >
                  {{ t(roleLabels[entry]) }}
                </SelectItem>
              </SelectContent>
            </Select>
          </div>
          <FormField
            id="invite-days"
            v-model="days"
            type="number"
            inputmode="numeric"
            :label="t('invitations.lifetime')"
            :suffix="t('units.days')"
            :description="t('invitations.lifetimeHint', { max: INVITATION_LIFETIME_MAX_DAYS })"
            :error="fieldMessage(invite.error.value, 'lifetime_days')"
            required
          />
          <CommandAlert
            :error="invite.error.value"
            :shown="['email', 'lifetime_days']"
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
              :loading="invite.isPending.value"
              :disabled="!valid"
            >
              {{ t('invitations.create') }}
            </Button>
          </div>
        </form>
      </template>
    </DialogContent>
  </Dialog>
</template>
