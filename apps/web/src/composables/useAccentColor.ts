// Adapted from holocron (84fc54ec): apps/web/src/composables/useAccentColor.ts.
import { createSharedComposable } from '@vueuse/core'
import { ref, watchEffect } from 'vue'

import type { AppAccent } from '~/generated'

/** The admin presets in `assets/css/tokens.css`, default first. */
export const accentColors = [
  'indigo',
  'ultraviolet',
  'magenta',
  'pink',
  'tangerine',
  'volt',
  'aqua',
  'azure',
] as const satisfies readonly AppAccent[]

/** Shared with the inline script in index.html, which applies the attribute before first paint. */
const STORAGE_KEY = 'v1per:accent'

const isAccentColor = (value: unknown): value is AppAccent => accentColors.some((accent) => accent === value)

const readStoredAccent = (): AppAccent => {
  try {
    const stored = window.localStorage.getItem(STORAGE_KEY)
    return isAccentColor(stored) ? stored : 'indigo'
  } catch {
    return 'indigo'
  }
}

const useAccentColorBase = () => {
  const accent = ref<AppAccent>(readStoredAccent())

  watchEffect(() => {
    document.documentElement.dataset.accent = accent.value
  })

  const setAccent = (next: AppAccent): void => {
    accent.value = next
    try {
      window.localStorage.setItem(STORAGE_KEY, next)
    } catch {
      // The accent still applies for this session.
    }
  }

  return { accent, setAccent, isAccentColor }
}

/** One source of truth: the picker and anything else reading it stay in step. */
export const useAccentColor = createSharedComposable(useAccentColorBase)
