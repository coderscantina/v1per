// Adapted from Folio (219e9aaa, MIT): apps/web/src/services/runtime-config.ts.
import { reactive } from 'vue'

import type { RuntimeConfig } from '~/generated'

/**
 * Deployment settings read at boot from `/config.json`, which the Rust server writes from its own
 * configuration. One build serves every installation; a changed setting needs a server restart,
 * not a rebuild. Read values at use time: `loadRuntimeConfig` replaces them during boot.
 */
export const runtimeConfig = reactive<RuntimeConfig>({
  api_base: '/api',
  version: '',
  instance_name: 'v1per',
})

function parse(value: unknown): Partial<RuntimeConfig> {
  if (typeof value !== 'object' || value === null) return {}
  const entries = Object.entries(value).filter(
    ([key, field]) => key in runtimeConfig && typeof field === 'string'
  )
  return Object.fromEntries(entries)
}

/** A missing or invalid file (a static host answering with HTML) keeps the defaults. */
export async function loadRuntimeConfig(): Promise<void> {
  try {
    const response = await fetch('/config.json', { cache: 'no-store' })
    if (!response.ok) return
    const loaded = parse(await response.json().catch(() => null))
    Object.assign(runtimeConfig, loaded)
    runtimeConfig.api_base = runtimeConfig.api_base.trim().replace(/\/+$/, '')
  } catch {
    // Offline or blocked: the defaults match the bundled server.
  }
}
