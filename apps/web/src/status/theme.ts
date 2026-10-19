import type { PageAppearance } from '~/generated'

const system = window.matchMedia('(prefers-color-scheme: dark)')
let theme: PageAppearance['theme'] = 'auto'

function paint() {
  const dark = theme === 'dark' || (theme === 'auto' && system.matches)
  const root = document.documentElement
  root.classList.toggle('dark', dark)
  root.style.colorScheme = dark ? 'dark' : 'light'
}

system.addEventListener('change', paint)

/** Applies the page's theme and accent to `<html>`. `auto` follows the visitor's system setting. */
export function applyAppearance(appearance: PageAppearance) {
  theme = appearance.theme
  document.documentElement.dataset.accent = appearance.accent
  paint()
}
