<script setup lang="ts">
import { useMutation } from '@tanstack/vue-query'

import Icon from '~/components/Icon.vue'
import { useErrorMessage } from '~/composables/useErrorMessage'
import { NAME_MAX_CHARS } from '~/generated/limits'
import type { IconName } from '~/lib/icons'

/**
 * A name typed in place as a row of the shell's tree, for a new folder or a rename. Enter or
 * leaving the field saves a changed name, Escape or leaving it empty or unchanged cancels.
 * `done` fires once `save` resolved or the edit is dropped; a refusal shows under the field.
 */
const props = defineProps<{
  depth: number
  icon: IconName
  label: string
  initial?: string
  save: (name: string) => Promise<unknown>
}>()
const emit = defineEmits<{ done: [] }>()

const { message, fieldMessage } = useErrorMessage()
const name = ref(props.initial ?? '')
const input = useTemplateRef('input')
const errorId = useId()

/** Set once the row goes, so the blur of a removed field does nothing. */
let closed = false
const close = () => {
  closed = true
  emit('done')
}

const run = useMutation({ mutationFn: (value: string) => props.save(value), onSuccess: close })

const error = computed(() => {
  const caught = run.error.value
  if (!caught) return null
  return fieldMessage(caught, 'name') ?? fieldMessage(caught, 'parent') ?? message(caught)
})

const submit = () => {
  if (run.isPending.value) return
  const value = name.value.trim()
  if (value === '' || value === props.initial) close()
  else run.mutate(value)
}

/** Leaving the field again after a refusal keeps it open instead of retrying the same name. */
const blur = () => {
  if (closed || run.isPending.value) return
  if (!run.error.value) submit()
  else if (name.value.trim() === '') close()
}

const edit = () => {
  if (run.error.value) run.reset()
}

onMounted(() => {
  input.value?.focus()
  input.value?.select()
})
</script>

<template>
  <li
    class="flex flex-col gap-0.5 py-0.5"
    :style="{ paddingLeft: `${depth * 14}px` }"
  >
    <div class="flex h-[26px] items-center">
      <span class="ml-0.5 w-4 shrink-0" />
      <div class="flex min-w-0 flex-1 items-center gap-[7px] pr-1 pl-[7px]">
        <Icon
          :name="icon"
          size="14"
          class="shrink-0 text-muted"
          aria-hidden="true"
        />
        <input
          ref="input"
          v-model="name"
          type="text"
          autocomplete="off"
          :maxlength="NAME_MAX_CHARS"
          :readonly="run.isPending.value"
          :aria-label="label"
          :aria-invalid="error ? 'true' : undefined"
          :aria-describedby="error ? errorId : undefined"
          :placeholder="label"
          class="h-[26px] min-w-0 flex-1 rounded-md border border-accent/60 bg-surface px-1.5 text-[0.8125rem] text-primary outline-none placeholder:text-muted focus-visible:outline-2 focus-visible:outline-offset-1 focus-visible:outline-ring aria-invalid:border-destructive"
          @input="edit"
          @keydown.enter.prevent="submit"
          @keydown.esc.prevent="close"
          @blur="blur"
        >
      </div>
    </div>
    <p
      v-if="error"
      :id="errorId"
      class="pr-1 pl-[46px] text-xs text-destructive"
      role="alert"
    >
      {{ error }}
    </p>
  </li>
</template>
