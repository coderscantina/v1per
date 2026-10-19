import { createRouter, createWebHistory, type RouteRecordRaw } from 'vue-router'

import { startNode } from '~/composables/useNodes'
import { loadSession } from '~/composables/useSession'
import { INVITATION_PATH } from '~/generated/limits'
import type { Mode } from '~/lib/modes'

declare module 'vue-router' {
  interface RouteMeta {
    /** Signed-out pages render without the shell; signed-in visitors bounce home. */
    guest?: boolean
    /** Without the shell, for anyone: signed in, signed out or enrolling. */
    public?: boolean
    /** The only page an enrollment-only session may open. */
    enrollment?: boolean
    /** Instance admins only; anyone else gets the not-found page. */
    instanceAdmin?: boolean
    /** Which of the two ways of working a node page belongs to. */
    mode?: Mode
  }
}

const routes: RouteRecordRaw[] = [
  { path: '/login', name: 'login', component: () => import('./pages/LoginPage.vue'), meta: { guest: true } },
  {
    path: '/login/verify',
    name: 'login-verify',
    component: () => import('./pages/TwoFactorChallengePage.vue'),
    meta: { guest: true },
  },
  { path: '/enroll', name: 'enroll', component: () => import('./pages/EnrollPage.vue'), meta: { enrollment: true } },
  { path: INVITATION_PATH, name: 'join', component: () => import('./pages/JoinPage.vue'), meta: { public: true } },
  {
    path: '/',
    name: 'home',
    component: () => import('./pages/HomePage.vue'),
    // Resolved before the shell renders, so opening the app is one navigation, not two.
    beforeEnter: async () => {
      const node = await startNode().catch(() => null)
      return node ? { name: 'node', params: { nodeId: node }, replace: true } : true
    },
  },
  { path: '/n/:nodeId(\\d+)', name: 'node', component: () => import('./pages/NodeOverviewPage.vue'), meta: { mode: 'observe' } },
  { path: '/n/:nodeId(\\d+)/monitors', name: 'node-monitors', component: () => import('./pages/MonitorListPage.vue'), meta: { mode: 'configure' } },
  {
    path: '/n/:nodeId(\\d+)/monitors/new',
    name: 'node-monitor-new',
    component: () => import('./pages/MonitorEditorPage.vue'),
    meta: { mode: 'configure' },
  },
  {
    path: '/n/:nodeId(\\d+)/monitors/:monitorId(\\d+)',
    name: 'node-monitor',
    component: () => import('./pages/MonitorDetailPage.vue'),
    meta: { mode: 'observe' },
  },
  {
    path: '/n/:nodeId(\\d+)/monitors/:monitorId(\\d+)/edit',
    name: 'node-monitor-edit',
    component: () => import('./pages/MonitorEditorPage.vue'),
    meta: { mode: 'configure' },
  },
  { path: '/n/:nodeId(\\d+)/incidents', name: 'node-incidents', component: () => import('./pages/NodeIncidentsPage.vue'), meta: { mode: 'observe' } },
  {
    path: '/n/:nodeId(\\d+)/incidents/new',
    name: 'node-incident-new',
    component: () => import('./pages/IncidentEditorPage.vue'),
    meta: { mode: 'observe' },
  },
  {
    path: '/n/:nodeId(\\d+)/incidents/:incidentId(\\d+)',
    name: 'node-incident',
    component: () => import('./pages/IncidentDetailPage.vue'),
    meta: { mode: 'observe' },
  },
  {
    path: '/n/:nodeId(\\d+)/incidents/:incidentId(\\d+)/edit',
    name: 'node-incident-edit',
    component: () => import('./pages/IncidentEditorPage.vue'),
    meta: { mode: 'observe' },
  },
  {
    path: '/n/:nodeId(\\d+)/maintenance',
    name: 'node-maintenance',
    component: () => import('./pages/NodeMaintenancePage.vue'),
    meta: { mode: 'configure' },
  },
  {
    path: '/n/:nodeId(\\d+)/maintenance/new',
    name: 'node-maintenance-new',
    component: () => import('./pages/MaintenanceEditorPage.vue'),
    meta: { mode: 'configure' },
  },
  {
    path: '/n/:nodeId(\\d+)/maintenance/:maintenanceId(\\d+)/edit',
    name: 'node-maintenance-edit',
    component: () => import('./pages/MaintenanceEditorPage.vue'),
    meta: { mode: 'configure' },
  },
  { path: '/n/:nodeId(\\d+)/channels', name: 'node-channels', component: () => import('./pages/NodeChannelsPage.vue'), meta: { mode: 'configure' } },
  {
    path: '/n/:nodeId(\\d+)/channels/new',
    name: 'node-channel-new',
    component: () => import('./pages/ChannelEditorPage.vue'),
    meta: { mode: 'configure' },
  },
  {
    path: '/n/:nodeId(\\d+)/channels/:channelId(\\d+)',
    name: 'node-channel',
    component: () => import('./pages/ChannelDetailPage.vue'),
    meta: { mode: 'configure' },
  },
  {
    path: '/n/:nodeId(\\d+)/channels/:channelId(\\d+)/edit',
    name: 'node-channel-edit',
    component: () => import('./pages/ChannelEditorPage.vue'),
    meta: { mode: 'configure' },
  },
  {
    path: '/n/:nodeId(\\d+)/components',
    name: 'node-components',
    component: () => import('./pages/NodeComponentsPage.vue'),
    meta: { mode: 'configure' },
  },
  { path: '/n/:nodeId(\\d+)/pages', name: 'node-pages', component: () => import('./pages/NodePagesPage.vue'), meta: { mode: 'configure' } },
  {
    path: '/n/:nodeId(\\d+)/pages/:pageId(\\d+)',
    component: () => import('./pages/page/PageEditorLayout.vue'),
    meta: { mode: 'configure' },
    // The page's own address opens its settings.
    redirect: (to) => ({ name: 'node-page', params: to.params }),
    children: [
      { path: 'settings', name: 'node-page', component: () => import('./pages/page/PageSettingsTab.vue') },
      { path: 'layout', name: 'node-page-layout', component: () => import('./pages/page/PageLayoutTab.vue') },
      { path: 'access', name: 'node-page-access', component: () => import('./pages/page/PageAccessTab.vue') },
      { path: 'hostnames', name: 'node-page-hostnames', component: () => import('./pages/page/PageHostnamesTab.vue') },
      { path: 'preview', name: 'node-page-preview', component: () => import('./pages/page/PagePreviewTab.vue') },
    ],
  },
  { path: '/n/:nodeId(\\d+)/members', name: 'node-members', component: () => import('./pages/NodeMembersPage.vue'), meta: { mode: 'configure' } },
  { path: '/n/:nodeId(\\d+)/probes', name: 'node-probes', component: () => import('./pages/NodeProbesPage.vue'), meta: { mode: 'configure' } },
  { path: '/n/:nodeId(\\d+)/settings', name: 'node-settings', component: () => import('./pages/NodeSettingsPage.vue'), meta: { mode: 'configure' } },
  {
    path: '/account',
    component: () => import('./pages/account/AccountLayout.vue'),
    children: [
      { path: '', redirect: { name: 'account-security' } },
      { path: 'security', name: 'account-security', component: () => import('./pages/account/SecurityPage.vue') },
      { path: 'api-keys', name: 'account-api-keys', component: () => import('./pages/account/ApiKeysPage.vue') },
    ],
  },
  {
    path: '/instance',
    name: 'instance-settings',
    component: () => import('./pages/InstanceSettingsPage.vue'),
    meta: { instanceAdmin: true },
  },
  {
    path: '/instance/imports',
    name: 'instance-imports',
    component: () => import('./pages/ImportsPage.vue'),
    meta: { instanceAdmin: true },
  },
  {
    // Not `nodeId`: that would make the import's node the shell's current node.
    path: '/instance/imports/:importNode(\\d+)',
    name: 'instance-import',
    component: () => import('./pages/ImportDetailPage.vue'),
    meta: { instanceAdmin: true },
  },
  { path: '/:path(.*)*', name: 'not-found', component: () => import('./pages/NotFoundPage.vue') },
]

export const router = createRouter({
  history: createWebHistory(),
  routes,
})

const withRedirect = (fullPath: string) => (fullPath === '/' ? {} : { redirect: fullPath })

/**
 * Every page but sign-in and the invitation link needs a session; `auth.session` answers
 * signed-out callers too. An enrollment-only session is held on the enrollment page.
 */
router.beforeEach(async (to) => {
  const session = await loadSession().catch(() => ({ account: null, enrollment_only: false }))
  const { account, enrollment_only } = session
  if (to.meta.public) return true
  if (to.meta.guest) return account ? { name: enrollment_only ? 'enroll' : 'home' } : true
  if (!account) return { name: 'login', query: withRedirect(to.fullPath) }
  if (to.meta.enrollment) return enrollment_only ? true : { name: 'home' }
  if (enrollment_only) return { name: 'enroll', query: withRedirect(to.fullPath) }
  if (to.meta.instanceAdmin && !account.instance_admin) {
    return { name: 'not-found', params: { path: to.path.split('/').filter(Boolean) }, replace: true }
  }
  return true
})
