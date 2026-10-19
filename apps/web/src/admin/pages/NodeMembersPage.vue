<script setup lang="ts">
// Adapted from holocron (84fc54ec): apps/web/src/pages/workspace/MembersSection.vue. Grants are
// per node and inherited, so each row says where the person's role comes from.
import { membersResource } from '~/api/resources/members'
import Icon from '~/components/Icon.vue'
import LiveIndicator from '~/components/LiveIndicator.vue'
import InviteDialog from '~/components/nodes/InviteDialog.vue'
import PageHeader from '~/components/PageHeader.vue'
import { Alert } from '~/components/ui/alert'
import { Badge } from '~/components/ui/badge'
import { Button } from '~/components/ui/button'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '~/components/ui/select'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '~/components/ui/table'
import TableEmptyRow from '~/components/ui/TableEmptyRow.vue'
import TableLoadingRow from '~/components/ui/TableLoadingRow.vue'
import { useConfirm } from '~/composables/useConfirm'
import { useDateFormat } from '~/composables/useDateFormat'
import { useErrorMessage } from '~/composables/useErrorMessage'
import { useCurrentNode, useNodeDetail, useNodeInvitations, useNodeMembers } from '~/composables/useNodes'
import { usePageMeta } from '~/composables/usePageMeta'
import { useReauth } from '~/composables/useReauth'
import { useSession } from '~/composables/useSession'
import type { Invitation, InvitationState, Member, Role } from '~/generated'
import { assignableRoles, can, grantSource, memberControls, type MemberRefusal } from '~/lib/access'
import { invitationStateLabels, roleLabels } from '~/lib/roles'
import { useI18n, type MessageKey } from '~/plugins/i18n'

const { t } = useI18n()
const { message } = useErrorMessage()
const { confirm } = useConfirm()
const { date, dateTime } = useDateFormat()
const reauth = useReauth()
const router = useRouter()
const { account } = useSession()
const { routeId, tree } = useCurrentNode()

const detail = useNodeDetail(routeId)
const node = computed(() => detail.data.value?.node ?? null)
const manage = computed(() => can(node.value, 'members.manage'))
const members = useNodeMembers(routeId)
const invitations = useNodeInvitations(() => (manage.value ? routeId.value : null))

usePageMeta(() => t('members.title'))

const inviting = ref(false)
const busy = ref<string | null>(null)
const notice = ref<string | null>(null)
const failure = ref<unknown>(null)

const rows = computed(() => {
  const current = node.value
  if (!current) return []
  return (members.data.value?.members ?? []).map((member) => ({
    member,
    controls: memberControls({
      actor: current.role,
      capabilities: current.capabilities,
      node: current.id,
      self: account.value?.id,
      member,
    }),
    source: grantSource(member, current.id),
  }))
})

const refusalLabels: Record<MemberRefusal, MessageKey | null> = {
  capability: null,
  inherited: null,
  self: 'members.refusal.self',
  rank: 'members.refusal.rank',
}

const nodeName = (id: string) => tree.index.value.byId.get(id)?.name ?? null
const roleOf = (role: Role) => t(roleLabels[role])

const run = async (key: string, action: () => Promise<string | null>) => {
  busy.value = key
  notice.value = null
  failure.value = null
  try {
    notice.value = await action()
  } catch (error) {
    failure.value = error
  } finally {
    busy.value = null
  }
}

const changeRole = (member: Member, value: unknown) => {
  const current = node.value
  const role = assignableRoles(current?.role).find((entry) => entry === value)
  if (!current || !role || role === member.role) return
  const args = { node: current.id, user: member.user, role }
  void run(member.user, async () => {
    const done = await reauth.run('members.updateRole', args, () => membersResource.updateRole(args))
    return done ? t('members.roleChanged', { name: member.name, role: roleOf(role) }) : null
  })
}

const remove = async (member: Member) => {
  const current = node.value
  if (!current) return
  const self = member.user === account.value?.id
  const ok = await confirm({
    title: self ? t('members.leave') : t('members.remove'),
    message: self
      ? t('members.leaveConfirm', { node: current.name })
      : t('members.removeConfirm', { name: member.name, node: current.name }),
    confirmLabel: self ? t('members.leave') : t('members.remove'),
    variant: 'destructive',
  })
  if (!ok) return
  void run(member.user, async () => {
    await membersResource.remove({ node: current.id, user: member.user })
    if (self) {
      await router.push({ name: 'home' })
      return null
    }
    return t('members.removed', { name: member.name })
  })
}

const revoke = async (invitation: Invitation) => {
  const ok = await confirm({
    title: t('invitations.revoke'),
    message: t('invitations.revokeConfirm', { email: invitation.email }),
    confirmLabel: t('invitations.revoke'),
    variant: 'destructive',
  })
  if (!ok) return
  void run(invitation.id, async () => {
    await membersResource.revokeInvitation(invitation.id)
    return t('invitations.revoked', { email: invitation.email })
  })
}

const stateVariants: Record<InvitationState, 'warning' | 'success' | 'secondary'> = {
  pending: 'warning',
  used: 'success',
  revoked: 'secondary',
  expired: 'secondary',
}
</script>

<template>
  <div>
    <PageHeader
      :title="t('members.title')"
      :description="node?.name"
    >
      <template
        v-if="manage && node"
        #actions
      >
        <Button
          variant="accent"
          @click="inviting = true"
        >
          <Icon name="lucide:user-plus" />
          {{ t('invitations.create') }}
        </Button>
      </template>
      <template
        v-if="members.data.value"
        #freshness
      >
        <LiveIndicator :status="members.status.value" />
      </template>
    </PageHeader>

    <Alert
      v-if="detail.error.value && !node"
      color="destructive"
      class="max-w-5xl"
    >
      <p class="text-sm">{{ message(detail.error.value) }}</p>
    </Alert>
    <div
      v-else
      class="grid max-w-5xl gap-8"
    >
      <Alert
        v-if="node && !manage"
        color="info"
      >
        <p class="text-sm">{{ t('members.readOnly') }}</p>
      </Alert>
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
          v-else-if="failure"
          color="destructive"
        >
          <p class="text-sm">{{ message(failure) }}</p>
        </Alert>
      </div>

      <section
        class="grid gap-3"
        aria-labelledby="members-title"
      >
        <h2
          id="members-title"
          class="flex items-center gap-2.5 text-[1.0625rem] font-semibold text-primary"
        >
          {{ t('members.people') }}
          <span
            v-if="members.data.value"
            class="text-sm font-normal text-muted tabular-nums"
          >· {{ members.data.value.members.length }}</span>
        </h2>
        <Table :label="t('members.people')">
          <TableHeader>
            <TableRow>
              <TableHead>{{ t('members.person') }}</TableHead>
              <TableHead>{{ t('members.role') }}</TableHead>
              <TableHead>{{ t('members.access') }}</TableHead>
              <TableHead><span class="sr-only">{{ t('actions.label') }}</span></TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            <TableLoadingRow
              v-if="!members.data.value && !members.error.value"
              :colspan="4"
              :rows="3"
            />
            <TableEmptyRow
              v-else-if="rows.length === 0"
              :colspan="4"
              :label="members.error.value ? message(members.error.value) : t('members.empty')"
            />
            <TableRow
              v-for="{ member, controls, source } in rows"
              :key="member.user"
            >
              <TableCell>
                <div class="grid min-w-0 gap-0.5">
                  <span class="flex items-center gap-1.5 font-medium text-primary">
                    <span class="truncate">{{ member.name }}</span>
                    <Badge
                      v-if="member.user === account?.id"
                      size="xs"
                    >
                      {{ t('members.you') }}
                    </Badge>
                    <Icon
                      v-if="member.two_factor_enrolled"
                      name="lucide:shield-check"
                      class="shrink-0 text-success"
                      role="img"
                      :aria-label="t('members.twoFactorOn')"
                    />
                  </span>
                  <span class="truncate text-xs text-muted">{{ member.email }}</span>
                </div>
              </TableCell>
              <TableCell class="min-w-36">
                <Select
                  v-if="controls.changeRole === null && node"
                  :model-value="member.role"
                  :disabled="busy === member.user"
                  @update:model-value="(value) => changeRole(member, value)"
                >
                  <SelectTrigger
                    class="h-8 w-36"
                    :aria-label="t('members.roleOf', { name: member.name })"
                  >
                    <SelectValue>{{ roleOf(member.role) }}</SelectValue>
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem
                      v-for="role in assignableRoles(node.role)"
                      :key="role"
                      :value="role"
                    >
                      {{ roleOf(role) }}
                    </SelectItem>
                  </SelectContent>
                </Select>
                <div
                  v-else
                  class="grid gap-0.5"
                >
                  <span class="text-primary">{{ roleOf(member.role) }}</span>
                  <span
                    v-if="manage && controls.changeRole && refusalLabels[controls.changeRole]"
                    class="text-xs text-muted"
                  >
                    {{ t(refusalLabels[controls.changeRole] ?? 'members.refusal.rank') }}
                  </span>
                </div>
              </TableCell>
              <TableCell class="min-w-40">
                <span
                  v-if="source.kind === 'here'"
                  class="text-primary"
                >{{ t('members.source.here') }}</span>
                <span
                  v-else-if="source.kind === 'instance_admin'"
                  class="text-primary"
                >{{ t('members.source.instanceAdmin') }}</span>
                <span
                  v-else-if="nodeName(source.node)"
                  class="text-muted"
                >
                  {{ t('members.source.from') }}
                  <RouterLink
                    :to="{ name: 'node-members', params: { nodeId: source.node } }"
                    class="rounded-sm text-primary underline-offset-4 hover:underline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring"
                  >
                    {{ nodeName(source.node) }}
                  </RouterLink>
                </span>
                <span
                  v-else
                  class="text-muted"
                >{{ t('members.source.above') }}</span>
              </TableCell>
              <TableCell class="text-right">
                <Button
                  v-if="controls.remove === null"
                  variant="ghost"
                  size="sm"
                  class="text-destructive"
                  :loading="busy === member.user"
                  :aria-label="member.user === account?.id ? t('members.leave') : t('members.removeNamed', { name: member.name })"
                  @click="remove(member)"
                >
                  <Icon :name="member.user === account?.id ? 'lucide:log-out' : 'lucide:user-minus'" />
                  <span class="max-sm:sr-only">
                    {{ member.user === account?.id ? t('members.leave') : t('members.remove') }}
                  </span>
                </Button>
              </TableCell>
            </TableRow>
          </TableBody>
        </Table>
        <p class="text-xs text-muted">{{ t('members.inheritHint') }}</p>
      </section>

      <section
        v-if="manage"
        class="grid gap-3"
        aria-labelledby="invitations-title"
      >
        <h2
          id="invitations-title"
          class="text-[1.0625rem] font-semibold text-primary"
        >
          {{ t('invitations.title') }}
        </h2>
        <Table :label="t('invitations.title')">
          <TableHeader>
            <TableRow>
              <TableHead>{{ t('invitations.email') }}</TableHead>
              <TableHead>{{ t('members.role') }}</TableHead>
              <TableHead>{{ t('invitations.state') }}</TableHead>
              <TableHead>{{ t('invitations.expires') }}</TableHead>
              <TableHead><span class="sr-only">{{ t('actions.label') }}</span></TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            <TableLoadingRow
              v-if="!invitations.data.value && !invitations.error.value"
              :colspan="5"
              :rows="2"
            />
            <TableEmptyRow
              v-else-if="(invitations.data.value?.invitations ?? []).length === 0"
              :colspan="5"
              :label="invitations.error.value ? message(invitations.error.value) : t('invitations.empty')"
            />
            <TableRow
              v-for="invitation in invitations.data.value?.invitations ?? []"
              :key="invitation.id"
            >
              <TableCell>
                <div class="grid min-w-0 gap-0.5">
                  <span class="truncate font-medium text-primary">{{ invitation.email }}</span>
                  <span class="font-mono text-xs text-muted">{{ invitation.prefix }}…</span>
                </div>
              </TableCell>
              <TableCell>{{ roleOf(invitation.role) }}</TableCell>
              <TableCell>
                <Badge
                  :variant="stateVariants[invitation.state]"
                  size="sm"
                >
                  {{ t(invitationStateLabels[invitation.state]) }}
                </Badge>
              </TableCell>
              <TableCell
                class="whitespace-nowrap text-muted"
                :title="dateTime(invitation.expires_at)"
              >
                {{ date(invitation.expires_at) }}
              </TableCell>
              <TableCell class="text-right">
                <Button
                  v-if="invitation.state === 'pending'"
                  variant="ghost"
                  size="icon"
                  :loading="busy === invitation.id"
                  :aria-label="t('invitations.revokeNamed', { email: invitation.email })"
                  @click="revoke(invitation)"
                >
                  <Icon name="lucide:trash-2" />
                </Button>
              </TableCell>
            </TableRow>
          </TableBody>
        </Table>
      </section>
    </div>

    <InviteDialog
      v-if="node && manage"
      v-model:open="inviting"
      :node="node"
    />
  </div>
</template>
