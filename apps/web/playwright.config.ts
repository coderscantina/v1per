import { defineConfig, devices } from '@playwright/test'

/** A server the suite starts itself (see `e2e/global-setup.ts`), unless `V1PER_E2E_URL` names one. */
export const e2ePort = Number(process.env.V1PER_E2E_PORT ?? 8095)

/**
 * Browser journeys against a real `v1per serve` with the built assets in `dist`. Run
 * `bun run build` first. With `V1PER_E2E_SHOTS` set, screenshots are written to that directory.
 */
export default defineConfig({
  testDir: './e2e',
  globalSetup: './e2e/global-setup.ts',
  fullyParallel: false,
  workers: 1,
  reporter: 'list',
  use: {
    baseURL: process.env.V1PER_E2E_URL ?? `http://127.0.0.1:${e2ePort}`,
    trace: 'retain-on-failure',
  },
  projects: [{ name: 'chromium', use: { ...devices['Desktop Chrome'] } }],
})
