<script setup lang="ts">
import { useMutation } from '@tanstack/vue-query'

import { monitorsResource } from '~/api/resources/monitors'
import SettingsPage from '~/components/account/SettingsPage.vue'
import SettingsSection from '~/components/account/SettingsSection.vue'
import ChannelPicker from '~/components/channels/ChannelPicker.vue'
import CommandAlert from '~/components/CommandAlert.vue'
import FormField from '~/components/FormField.vue'
import Icon from '~/components/Icon.vue'
import HeadersEditor from '~/components/monitors/HeadersEditor.vue'
import ProbePicker from '~/components/monitors/ProbePicker.vue'
import PushUrl from '~/components/monitors/PushUrl.vue'
import SecretInputField from '~/components/monitors/SecretInputField.vue'
import PageHeader from '~/components/PageHeader.vue'
import SelectField from '~/components/SelectField.vue'
import SwitchField from '~/components/SwitchField.vue'
import TextareaField from '~/components/TextareaField.vue'
import { Alert } from '~/components/ui/alert'
import { Button } from '~/components/ui/button'
import { Skeleton } from '~/components/ui/skeleton'
import { useAvailableChannels } from '~/composables/useChannels'
import { useConfirm } from '~/composables/useConfirm'
import { useErrorMessage } from '~/composables/useErrorMessage'
import { useItemSections } from '~/composables/useItemSections'
import { useMonitorDetail, useMonitorsOf } from '~/composables/useMonitors'
import { useCurrentNode, useNodeDetail } from '~/composables/useNodes'
import { usePageMeta } from '~/composables/usePageMeta'
import { useAvailableProbes } from '~/composables/useProbes'
import type { HttpAuth, IpFamily, MonitorResult, OAuthClientAuth } from '~/generated'
import { PUSH_TOKEN_MAX_CHARS, PUSH_TOKEN_MIN_CHARS } from '~/generated/limits'
import { can } from '~/lib/access'
import {
  authMethods,
  bodyEncodings,
  creatableTypes,
  dnsRecordTypes,
  emptyForm,
  formFromMonitor,
  httpMethods,
  jsonOperators,
  newPushToken,
  oauthClientAuths,
  quorumApplies,
  quorums,
  runsOnProbes,
  sendsRequest,
  toInput,
  validate,
  type CreatableType,
  type MonitorForm,
} from '~/lib/monitor-form'
import { quorumLabels, typeLabels } from '~/lib/monitor-status'
import { isReadable } from '~/lib/node-tree'
import { useI18n, type MessageKey } from '~/plugins/i18n'

/**
 * Adds a monitor to the node, or edits one. Every type keeps its own fields, so switching type
 * loses nothing typed. The form reads the monitor once; later changes by others surface as a
 * stale revision on save, with a reload. Saving an edit stays here, in configure mode; a new
 * monitor opens its page to show its first checks.
 */
const { t } = useI18n()
const { confirm } = useConfirm()
const { fieldMessage, fieldProblem } = useErrorMessage()
const route = useRoute()
const router = useRouter()
const { routeId, path } = useCurrentNode()
const node = useNodeDetail(routeId)
const manage = computed(() => can(node.data.value?.node, 'monitors.manage'))

const monitorId = computed(() => (typeof route.params.monitorId === 'string' ? route.params.monitorId : null))
const editing = computed(() => monitorId.value !== null)
const existing = useMonitorDetail(monitorId)
const stored = computed(() => existing.data.value?.monitor ?? null)
usePageMeta(() => (editing.value ? t('monitors.editor.editTitle') : t('monitors.editor.createTitle')))

const form = ref<MonitorForm>(emptyForm())
/** The revision the form was read at; `null` until it is read. */
const basedOn = ref<number | null>(null)
const unsupported = ref(false)

const load = () => {
  const monitor = stored.value
  if (!monitor) return
  const next = formFromMonitor(monitor)
  unsupported.value = next === null
  if (next) form.value = next
  basedOn.value = monitor.revision
}
watch(stored, () => {
  if (basedOn.value === null) load()
}, { immediate: true })

/** Group monitors on this node and on the readable nodes above it, as the server accepts. */
const NO_GROUP = 'none'
const groupNodes = computed(() => path.value.filter(isReadable).map((entry) => entry.id))
const groupLists = useMonitorsOf(groupNodes)
const groupOptions = computed(() => {
  const options = groupNodes.value.flatMap((nodeId, at) => {
    const name = path.value.find((entry) => entry.id === nodeId)?.name
    return (groupLists.value.lists[at] ?? [])
      .filter((monitor) => monitor.type === 'group' && monitor.id !== monitorId.value)
      .map((monitor) => ({ value: monitor.id, label: nodeId === routeId.value ? monitor.name : `${monitor.name} · ${name ?? ''}` }))
  })
  return [{ value: NO_GROUP, label: t('monitors.editor.noGroup') }, ...options]
})
const group = computed({
  get: () => form.value.group ?? NO_GROUP,
  set: (value: string) => (form.value.group = value === NO_GROUP ? null : value),
})

const available = useAvailableProbes(routeId)
const probeChoices = computed(() => available.data.value?.probes ?? [])
const channels = useAvailableChannels(routeId)
const channelChoices = computed(() => channels.data.value?.channels ?? [])

const submitted = ref(false)
const clientErrors = computed(() => (submitted.value ? validate(form.value) : {}))

const save = useMutation({
  mutationFn: (current: MonitorForm): Promise<MonitorResult> => {
    const input = toInput(current)
    const monitor = monitorId.value
    if (monitor) return monitorsResource.update({ ...input, monitor, expected_revision: basedOn.value ?? undefined })
    return monitorsResource.create({ ...input, node: routeId.value ?? '' })
  },
})

const errorFor = (key: string): string | null => {
  const local = clientErrors.value[key]
  if (local) return fieldProblem(local)
  return fieldMessage(save.error.value, key)
}

/** A push token's length problem says the whole range; the server's other answers stay as they are. */
const pushTokenError = computed(() =>
  clientErrors.value['settings.push_token']
    ? t('monitors.editor.pushTokenLength', { min: PUSH_TOKEN_MIN_CHARS, max: PUSH_TOKEN_MAX_CHARS })
    : errorFor('settings.push_token')
)

const saved = ref(false)
watch(form, () => (saved.value = false), { deep: true })

const submit = () => {
  submitted.value = true
  if (Object.keys(validate(form.value)).length > 0) {
    void nextTick(() => document.querySelector<HTMLElement>('[aria-invalid="true"]')?.focus())
    return
  }
  save.mutate(form.value, {
    onSuccess: ({ monitor }) => {
      if (!editing.value) {
        void router.push({ name: 'node-monitor', params: { nodeId: monitor.node, monitorId: monitor.id } })
        return
      }
      const next = formFromMonitor(monitor)
      if (next) form.value = next
      basedOn.value = monitor.revision
      submitted.value = false
      void nextTick(() => (saved.value = true))
    },
  })
}

const remove = useMutation({ mutationFn: (monitor: string) => monitorsResource.delete(monitor) })
const confirmRemove = async () => {
  const monitor = stored.value
  if (!monitor) return
  const ok = await confirm({
    title: t('monitors.editor.deleteSection'),
    message: t('monitors.detail.deleteConfirm', { name: monitor.name }),
    confirmLabel: t('monitors.detail.delete'),
    variant: 'destructive',
  })
  if (!ok) return
  remove.mutate(monitor.id, {
    onSuccess: () => void router.replace({ name: 'node-monitors', params: { nodeId: monitor.node } }),
  })
}

const reload = async () => {
  await existing.reload()
  basedOn.value = null
  load()
  save.reset()
}

const shownFields = [
  'name',
  'description',
  'tags',
  'group',
  'interval',
  'retry_interval',
  'max_retries',
  'resend_interval',
  'timeout',
  'quorum',
  'probes',
  'channels',
  'settings.expiry_notification',
  'settings.url',
  'settings.method',
  'settings.headers',
  'settings.accepted_statuscodes',
  'settings.maxredirects',
  'settings.auth.basic_auth_pass',
  'settings.auth.bearer_token',
  'settings.auth.oauth_token_url',
  'settings.auth.oauth_client_id',
  'settings.auth.oauth_client_secret',
  'settings.keyword',
  'settings.hostname',
  'settings.port',
  'settings.ping_count',
  'settings.packet_size',
  'settings.ping_per_request_timeout',
  'settings.dns_resolve_server',
  'settings.push_token',
]

const typeNotes: Record<CreatableType, MessageKey> = {
  http: 'monitors.editor.typeNotes.http',
  keyword: 'monitors.editor.typeNotes.keyword',
  'json-query': 'monitors.editor.typeNotes.json-query',
  port: 'monitors.editor.typeNotes.port',
  ping: 'monitors.editor.typeNotes.ping',
  dns: 'monitors.editor.typeNotes.dns',
  push: 'monitors.editor.typeNotes.push',
  group: 'monitors.editor.typeNotes.group',
  manual: 'monitors.editor.typeNotes.manual',
}
const authLabels: Record<HttpAuth['auth_method'], MessageKey> = {
  none: 'monitors.editor.authMethods.none',
  basic: 'monitors.editor.authMethods.basic',
  bearer: 'monitors.editor.authMethods.bearer',
  'oauth2-cc': 'monitors.editor.authMethods.oauth2-cc',
}
const clientAuthLabels: Record<OAuthClientAuth, MessageKey> = {
  client_secret_basic: 'monitors.editor.oauthClientAuths.client_secret_basic',
  client_secret_post: 'monitors.editor.oauthClientAuths.client_secret_post',
}
const ipLabels: Record<IpFamily | 'any', MessageKey> = {
  any: 'monitors.editor.ipFamilies.any',
  ipv4: 'monitors.editor.ipFamilies.ipv4',
  ipv6: 'monitors.editor.ipFamilies.ipv6',
}

const options = <V extends string>(values: readonly V[], label: (value: V) => string) =>
  values.map((value) => ({ value, label: label(value) }))
const typeOptions = computed(() => options(creatableTypes, (type) => t(typeLabels[type])))
const methodOptions = options(httpMethods, (method) => method)
const encodingOptions = options(bodyEncodings, (encoding) => encoding.toUpperCase())
const authOptions = computed(() => options(authMethods, (method) => t(authLabels[method])))
const clientAuthOptions = computed(() => options(oauthClientAuths, (method) => t(clientAuthLabels[method])))
const operatorOptions = options(jsonOperators, (operator) => operator)
const recordOptions = options(dnsRecordTypes, (record) => record)
const ipOptions = computed(() => options(['any', 'ipv4', 'ipv6'] as const, (family) => t(ipLabels[family])))
const quorumOptions = computed(() => options(quorums, (quorum) => t(quorumLabels[quorum])))

const checked = computed(() => runsOnProbes(form.value.type))
const storedToken = computed(() => {
  const settings = stored.value?.settings
  return settings?.type === 'push' ? (settings.push_token ?? null) : null
})
const ready = computed(() => node.data.value && (!editing.value || basedOn.value !== null))
const backTo = computed(() => ({ name: 'node-monitors', params: { nodeId: routeId.value } }))

/** Empty fields fall back to these on the server, so they show as placeholders. */
const retryPlaceholder = computed(() => form.value.interval.trim())
const timeoutPlaceholder = computed(() => {
  const interval = Number(form.value.interval)
  return Number.isFinite(interval) && interval > 0 ? String(Math.round(interval * 0.8 * 10) / 10) : ''
})

const editorSections = computed(() => {
  if (!ready.value || !manage.value || unsupported.value) return []
  const type = form.value.type
  return [
    { id: 'monitor-basics', label: t('monitors.editor.basics') },
    ...(type !== 'group' && type !== 'manual'
      ? [{ id: 'monitor-check', label: type === 'push' ? t('monitors.editor.push') : t('monitors.editor.check') }]
      : []),
    ...(sendsRequest(type) ? [{ id: 'monitor-request', label: t('monitors.editor.request') }] : []),
    { id: 'monitor-schedule', label: t('monitors.editor.schedule') },
    ...(runsOnProbes(type) ? [{ id: 'monitor-probes', label: t('monitors.editor.probesSection') }] : []),
    { id: 'monitor-notifications', label: t('monitors.editor.notifications') },
  ]
})
useItemSections(editorSections)
</script>

<template>
  <div>
    <PageHeader
      :title="editing ? (stored?.name ?? t('monitors.editor.editTitle')) : t('monitors.editor.createTitle')"
      :description="editing ? t('monitors.editor.editTitle') : node.data.value?.node.name"
    />

    <Alert
      v-if="(node.error.value && !node.data.value) || (existing.error.value && !stored)"
      color="destructive"
      class="max-w-4xl"
    >
      <p class="text-sm">{{ t('monitors.unavailable') }}</p>
    </Alert>
    <div
      v-else-if="!ready"
      class="grid max-w-4xl gap-6"
    >
      <Skeleton class="h-32 w-full rounded-panel" />
      <Skeleton class="h-56 w-full rounded-panel" />
    </div>
    <Alert
      v-else-if="!manage"
      color="info"
      class="max-w-4xl"
    >
      <p class="text-sm">{{ t('monitors.readOnly') }}</p>
    </Alert>
    <Alert
      v-else-if="unsupported"
      color="warning"
      class="max-w-4xl"
    >
      <p class="text-sm">{{ t('monitors.editor.unsupported') }}</p>
    </Alert>

    <form
      v-else
      class="max-w-4xl"
      novalidate
      @submit.prevent="submit"
    >
      <SettingsPage>
        <SettingsSection
          id="monitor-basics"
          :title="t('monitors.editor.basics')"
          :description="t(typeNotes[form.type])"
        >
          <SelectField
            id="monitor-type-select"
            v-model="form.type"
            :label="t('monitors.editor.type')"
            :options="typeOptions"
            :error="errorFor('settings')"
          />
          <FormField
            id="monitor-name"
            v-model="form.name"
            :label="t('monitors.editor.name')"
            :error="errorFor('name')"
            required
          />
          <TextareaField
            id="monitor-description"
            v-model="form.description"
            :rows="2"
            :label="t('monitors.editor.description')"
            :error="errorFor('description')"
          />
          <div class="grid gap-4 sm:grid-cols-2">
            <FormField
              id="monitor-tags"
              v-model="form.tags"
              :label="t('monitors.editor.tags')"
              :placeholder="t('monitors.editor.tagsPlaceholder')"
              :error="errorFor('tags')"
            />
            <SelectField
              id="monitor-group"
              v-model="group"
              :label="t('monitors.editor.group')"
              :options="groupOptions"
              :error="errorFor('group')"
            />
          </div>
          <SwitchField
            v-if="form.type === 'group' || form.type === 'manual'"
            id="monitor-upside-down"
            v-model="form.upsideDown"
            :label="t('monitors.editor.upsideDown')"
            :description="t('monitors.editor.upsideDownHint')"
          />
        </SettingsSection>

        <SettingsSection
          v-if="form.type !== 'group' && form.type !== 'manual'"
          id="monitor-check"
          :title="form.type === 'push' ? t('monitors.editor.push') : t('monitors.editor.check')"
        >
          <div
            v-if="sendsRequest(form.type)"
            class="grid gap-4 sm:grid-cols-[8rem_minmax(0,1fr)]"
          >
            <SelectField
              id="monitor-method"
              v-model="form.request.method"
              :label="t('monitors.editor.method')"
              :options="methodOptions"
              :error="errorFor('settings.method')"
            />
            <FormField
              id="monitor-url"
              v-model="form.request.url"
              type="url"
              inputmode="url"
              :label="t('monitors.editor.url')"
              :error="errorFor('settings.url')"
              required
            />
          </div>
          <template v-if="form.type === 'keyword'">
            <FormField
              id="monitor-keyword"
              v-model="form.keyword"
              :label="t('monitors.editor.keyword')"
              :description="t('monitors.editor.keywordHint')"
              :error="errorFor('settings.keyword')"
              required
            />
            <SwitchField
              id="monitor-invert-keyword"
              v-model="form.invertKeyword"
              :label="t('monitors.editor.invertKeyword')"
            />
          </template>
          <template v-if="form.type === 'json-query'">
            <FormField
              id="monitor-json-path"
              v-model="form.jsonPath"
              :label="t('monitors.editor.jsonPath')"
              :description="t('monitors.editor.jsonPathHint')"
              :error="errorFor('settings.json_path')"
            />
            <div class="grid gap-4 sm:grid-cols-[10rem_1fr]">
              <SelectField
                id="monitor-json-operator"
                v-model="form.jsonOperator"
                :label="t('monitors.editor.operator')"
                :options="operatorOptions"
              />
              <FormField
                id="monitor-expected-value"
                v-model="form.expectedValue"
                :label="t('monitors.editor.expectedValue')"
                :error="errorFor('settings.expected_value')"
              />
            </div>
            <SwitchField
              id="monitor-retry-only-status"
              v-model="form.retryOnlyOnStatusCodeFailure"
              :label="t('monitors.editor.retryOnlyOnStatusCodeFailure')"
            />
          </template>
          <div
            v-if="form.type === 'port' || form.type === 'ping' || form.type === 'dns'"
            class="grid gap-4"
            :class="form.type === 'port' && 'sm:grid-cols-[minmax(0,1fr)_8rem]'"
          >
            <FormField
              id="monitor-hostname"
              v-model="form.hostname"
              :label="form.type === 'dns' ? t('monitors.editor.hostnameDns') : t('monitors.editor.hostname')"
              :error="errorFor('settings.hostname')"
              autocomplete="off"
              required
            />
            <FormField
              v-if="form.type === 'port'"
              id="monitor-port"
              v-model="form.port"
              inputmode="numeric"
              :label="t('monitors.editor.port')"
              :error="errorFor('settings.port')"
              required
            />
          </div>
          <div
            v-if="form.type === 'ping'"
            class="grid gap-4 sm:grid-cols-3"
          >
            <FormField
              id="monitor-ping-count"
              v-model="form.pingCount"
              inputmode="numeric"
              :label="t('monitors.editor.pingCount')"
              :error="errorFor('settings.ping_count')"
            />
            <FormField
              id="monitor-packet-size"
              v-model="form.packetSize"
              inputmode="numeric"
              :label="t('monitors.editor.packetSize')"
              :suffix="t('units.bytes')"
              :error="errorFor('settings.packet_size')"
            />
            <FormField
              id="monitor-ping-timeout"
              v-model="form.pingTimeout"
              inputmode="decimal"
              :label="t('monitors.editor.pingTimeout')"
              :suffix="t('units.seconds')"
              :error="errorFor('settings.ping_per_request_timeout')"
            />
          </div>
          <template v-if="form.type === 'dns'">
            <div class="grid gap-4 sm:grid-cols-[8rem_minmax(0,1fr)_8rem]">
              <SelectField
                id="monitor-dns-type"
                v-model="form.dnsRecordType"
                :label="t('monitors.editor.dnsRecordType')"
                :options="recordOptions"
              />
              <FormField
                id="monitor-dns-server"
                v-model="form.dnsServer"
                :label="t('monitors.editor.dnsServer')"
                :placeholder="t('monitors.editor.dnsServerPlaceholder')"
                :error="errorFor('settings.dns_resolve_server')"
              />
              <FormField
                id="monitor-dns-port"
                v-model="form.dnsPort"
                inputmode="numeric"
                :label="t('monitors.editor.dnsPort')"
                :error="errorFor('settings.port')"
              />
            </div>
          </template>
          <template v-if="form.type === 'push'">
            <p
              v-if="!editing"
              class="text-sm text-muted"
            >
              {{ t('monitors.editor.pushNew') }}
            </p>
            <template v-else-if="form.pushToken ?? storedToken">
              <PushUrl
                id="monitor-push-url"
                :token="form.pushToken ?? storedToken ?? ''"
              />
              <p
                v-if="form.pushToken"
                class="text-sm text-warning-foreground"
              >
                {{ t('monitors.editor.newTokenHint') }}
              </p>
            </template>
            <p
              v-if="pushTokenError"
              class="text-xs font-medium text-destructive"
            >
              {{ pushTokenError }}
            </p>
            <Button
              v-if="editing"
              type="button"
              variant="fill"
              size="sm"
              class="justify-self-start"
              @click="form.pushToken = newPushToken()"
            >
              {{ t('monitors.editor.newToken') }}
            </Button>
          </template>
          <SwitchField
            id="monitor-upside-down"
            v-model="form.upsideDown"
            :label="t('monitors.editor.upsideDown')"
            :description="t('monitors.editor.upsideDownHint')"
          />
        </SettingsSection>

        <SettingsSection
          v-if="sendsRequest(form.type)"
          id="monitor-request"
          :title="t('monitors.editor.request')"
        >
          <HeadersEditor
            id="monitor-headers"
            v-model="form.request.headers"
            :error="errorFor('settings.headers')"
          />
          <div class="grid gap-4 sm:grid-cols-[minmax(0,1fr)_10rem]">
            <TextareaField
              id="monitor-body"
              v-model="form.request.body"
              :rows="3"
              :label="t('monitors.editor.body')"
              code
            />
            <SelectField
              id="monitor-body-encoding"
              v-model="form.request.bodyEncoding"
              :label="t('monitors.editor.bodyEncoding')"
              :options="encodingOptions"
            />
          </div>
          <div class="grid gap-4 sm:grid-cols-2">
            <SelectField
              id="monitor-auth"
              v-model="form.request.authMethod"
              :label="t('monitors.editor.auth')"
              :options="authOptions"
            />
            <SelectField
              id="monitor-ip-family"
              v-model="form.request.ipFamily"
              :label="t('monitors.editor.ipFamily')"
              :options="ipOptions"
            />
          </div>
          <div
            v-if="form.request.authMethod === 'basic'"
            class="grid gap-4 sm:grid-cols-2"
          >
            <FormField
              id="monitor-basic-user"
              v-model="form.request.basicUser"
              autocomplete="off"
              :label="t('monitors.editor.basicUser')"
            />
            <SecretInputField
              id="monitor-basic-pass"
              v-model="form.request.basicPass"
              :label="t('monitors.editor.basicPass')"
              :error="errorFor('settings.auth.basic_auth_pass')"
            />
          </div>
          <SecretInputField
            v-if="form.request.authMethod === 'bearer'"
            id="monitor-bearer"
            v-model="form.request.bearerToken"
            :label="t('monitors.editor.bearerToken')"
            :error="errorFor('settings.auth.bearer_token')"
          />
          <template v-if="form.request.authMethod === 'oauth2-cc'">
            <FormField
              id="monitor-oauth-url"
              v-model="form.request.oauthTokenUrl"
              type="url"
              :label="t('monitors.editor.oauthTokenUrl')"
              :error="errorFor('settings.auth.oauth_token_url')"
              required
            />
            <div class="grid gap-4 sm:grid-cols-2">
              <FormField
                id="monitor-oauth-client"
                v-model="form.request.oauthClientId"
                autocomplete="off"
                :label="t('monitors.editor.oauthClientId')"
                :error="errorFor('settings.auth.oauth_client_id')"
                required
              />
              <SecretInputField
                id="monitor-oauth-secret"
                v-model="form.request.oauthClientSecret"
                :label="t('monitors.editor.oauthClientSecret')"
                :error="errorFor('settings.auth.oauth_client_secret')"
              />
              <FormField
                id="monitor-oauth-scopes"
                v-model="form.request.oauthScopes"
                :label="t('monitors.editor.oauthScopes')"
              />
              <FormField
                id="monitor-oauth-audience"
                v-model="form.request.oauthAudience"
                :label="t('monitors.editor.oauthAudience')"
              />
            </div>
            <SelectField
              id="monitor-oauth-auth"
              v-model="form.request.oauthClientAuth"
              :label="t('monitors.editor.oauthClientAuth')"
              :options="clientAuthOptions"
            />
          </template>
          <div class="grid gap-4 sm:grid-cols-[minmax(0,1fr)_10rem]">
            <FormField
              id="monitor-accepted"
              v-model="form.request.acceptedStatusCodes"
              :label="t('monitors.editor.acceptedStatusCodes')"
              :placeholder="t('monitors.editor.acceptedPlaceholder')"
              :error="errorFor('settings.accepted_statuscodes')"
            />
            <FormField
              id="monitor-redirects"
              v-model="form.request.maxRedirects"
              inputmode="numeric"
              :label="t('monitors.editor.maxRedirects')"
              :error="errorFor('settings.maxredirects')"
            />
          </div>
          <SwitchField
            id="monitor-ignore-tls"
            v-model="form.request.ignoreTls"
            :label="t('monitors.editor.ignoreTls')"
          />
          <SwitchField
            id="monitor-cache-bust"
            v-model="form.request.cacheBust"
            :label="t('monitors.editor.cacheBust')"
          />
        </SettingsSection>

        <SettingsSection
          id="monitor-schedule"
          :title="t('monitors.editor.schedule')"
          :description="t('monitors.editor.scheduleHint')"
        >
          <div class="grid gap-4 sm:grid-cols-2">
            <FormField
              id="monitor-interval"
              v-model="form.interval"
              inputmode="numeric"
              :label="t('monitors.editor.interval')"
              :suffix="t('units.seconds')"
              :error="errorFor('interval')"
              required
            />
            <FormField
              v-if="checked"
              id="monitor-timeout"
              v-model="form.timeout"
              inputmode="decimal"
              :label="t('monitors.editor.timeout')"
              :suffix="t('units.seconds')"
              :placeholder="timeoutPlaceholder"
              :error="errorFor('timeout')"
            />
            <FormField
              id="monitor-max-retries"
              v-model="form.maxRetries"
              inputmode="numeric"
              :label="t('monitors.editor.maxRetries')"
              :error="errorFor('max_retries')"
            />
            <FormField
              id="monitor-retry-interval"
              v-model="form.retryInterval"
              inputmode="numeric"
              :label="t('monitors.editor.retryInterval')"
              :suffix="t('units.seconds')"
              :placeholder="retryPlaceholder"
              :error="errorFor('retry_interval')"
            />
            <FormField
              id="monitor-resend"
              v-model="form.resendInterval"
              inputmode="numeric"
              :label="t('monitors.editor.resendInterval')"
              :suffix="t('units.checks')"
              :error="errorFor('resend_interval')"
            />
          </div>
        </SettingsSection>

        <SettingsSection
          v-if="checked"
          id="monitor-probes"
          :title="t('monitors.editor.probesSection')"
          :description="t('monitors.editor.probesHint')"
        >
          <ProbePicker
            v-model="form.probes"
            :choices="probeChoices"
            :error="errorFor('probes')"
          />
          <SelectField
            v-if="quorumApplies(form, probeChoices)"
            id="monitor-quorum"
            v-model="form.quorum"
            class="sm:max-w-[calc(50%-0.5rem)]"
            :label="t('monitors.editor.quorum')"
            :error="errorFor('quorum')"
            :options="quorumOptions"
          />
        </SettingsSection>

        <SettingsSection
          id="monitor-notifications"
          :title="t('monitors.editor.notifications')"
          :description="t('monitors.editor.channelsHint')"
        >
          <ChannelPicker
            v-model="form.channels"
            :choices="channelChoices"
            :error="errorFor('channels')"
          />
          <SwitchField
            v-if="sendsRequest(form.type)"
            id="monitor-expiry-notification"
            v-model="form.request.expiryNotification"
            :label="t('monitors.editor.expiryNotification')"
            :description="form.request.ignoreTls ? t('monitors.editor.expiryNotificationIgnored') : undefined"
          />
        </SettingsSection>

        <SettingsSection
          v-if="editing && stored"
          id="monitor-delete"
          :title="t('monitors.editor.deleteSection')"
          destructive
        >
          <div class="flex flex-wrap items-center justify-between gap-3">
            <p class="text-sm text-muted">{{ t('monitors.editor.deleteText') }}</p>
            <Button
              type="button"
              variant="destructive"
              :loading="remove.isPending.value"
              @click="confirmRemove"
            >
              <Icon name="lucide:trash-2" />
              {{ t('monitors.detail.delete') }}
            </Button>
          </div>
        </SettingsSection>
      </SettingsPage>

      <div class="mt-8 grid gap-4 border-t border-divider pt-6">
        <Alert
          v-if="submitted && Object.keys(clientErrors).length > 0"
          color="destructive"
        >
          <p class="text-sm">{{ t('monitors.editor.invalid') }}</p>
        </Alert>
        <CommandAlert
          v-else-if="save.error.value || remove.error.value"
          :error="save.error.value ?? remove.error.value"
          :shown="shownFields"
          @reload="reload"
        />
        <Alert
          v-else-if="saved"
          color="success"
          role="status"
        >
          <p class="text-sm">{{ t('monitors.editor.saved') }}</p>
        </Alert>
        <div class="flex flex-wrap gap-2">
          <Button
            type="submit"
            variant="accent"
            :loading="save.isPending.value"
          >
            {{ editing ? t('monitors.editor.saveEdit') : t('monitors.editor.saveCreate') }}
          </Button>
          <Button
            as-child
            variant="fill"
          >
            <RouterLink :to="backTo">{{ t('actions.cancel') }}</RouterLink>
          </Button>
        </div>
      </div>
    </form>
  </div>
</template>
