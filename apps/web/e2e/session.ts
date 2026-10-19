import { join } from 'node:path'

/** The instance admin. `global-setup.ts` creates it, or reads it from the environment for an external server. */
export const admin = {
  email: process.env.V1PER_E2E_EMAIL ?? 'admin@example.test',
  get password(): string {
    return process.env.V1PER_E2E_PASSWORD ?? ''
  },
}

/** Where `global-setup.ts` stores the admin's signed-in browser state. */
export const sessionFile = process.env.V1PER_E2E_SESSION ?? join(import.meta.dirname, '.auth', 'admin.json')

/** The instance name the suite's own server runs with. */
export const instanceName = process.env.V1PER_E2E_INSTANCE_NAME ?? 'Rockhouse monitoring'
