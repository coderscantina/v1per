import { onScopeDispose, readonly, shallowRef, toValue, watchEffect, type MaybeRefOrGetter } from 'vue'

/** One section of an item's editor, as a jump link in the configure bar. */
export interface ItemSection {
  /** The section element's `id`. */
  id: string
  label: string
}

const current = shallowRef<readonly ItemSection[]>([])

/**
 * The sections of the item editor on screen, such as a monitor's, for the configure bar to
 * list in place of the node's tabs. An editor passes its sections while it is mounted; the bar
 * calls it without arguments to read them.
 */
export function useItemSections(sections?: MaybeRefOrGetter<readonly ItemSection[]>) {
  if (sections !== undefined) {
    let mine: readonly ItemSection[] = []
    watchEffect(() => (current.value = mine = toValue(sections)))
    // The next editor may already have set its own.
    onScopeDispose(() => {
      if (current.value === mine) current.value = []
    })
  }
  return readonly(current)
}
