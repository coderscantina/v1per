// From Kit (f447e797, MIT): resources/js/composables/useCommandPalette.ts.
import { ref } from 'vue'

/** Open state of the single palette mounted in App.vue, shared by its trigger and shortcut. */
const isOpen = ref(false)

export function useCommandPalette() {
  return {
    isOpen,
    open: (): void => {
      isOpen.value = true
    },
    close: (): void => {
      isOpen.value = false
    },
    toggle: (): void => {
      isOpen.value = !isOpen.value
    },
  }
}
