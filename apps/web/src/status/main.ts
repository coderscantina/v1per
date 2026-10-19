/**
 * The public status page entry, built to `dist/status.html` beside the admin app's `index.html`.
 * The server answers `/s/<slug>` and claimed hostnames with it and injects the page's boot JSON,
 * so the first render needs no request.
 */
import { createApp } from 'vue'

import { readBoot } from './boot'
import { locale } from './messages'
import StatusApp from './StatusApp.vue'
import { applyAppearance } from './theme'

import '~/assets/css/status.css'
import './status.css'

const boot = readBoot()
if (boot) {
  applyAppearance(boot.kind === 'page' ? boot.snapshot.appearance : boot.gate.appearance)
  locale.value = boot.kind === 'page' ? boot.snapshot.locale : boot.gate.locale
}

createApp(StatusApp, { boot }).mount('#status')
