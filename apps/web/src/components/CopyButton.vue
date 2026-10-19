<script setup lang="ts">
import Icon from '~/components/Icon.vue'
import { Button } from '~/components/ui/button'
import { useI18n } from '~/plugins/i18n'

/** Copies text and says so on the button for two seconds; the label change is announced. */
const props = defineProps<{ text: string; label?: string }>()

const { t } = useI18n()
const copied = ref(false)
let timer: ReturnType<typeof setTimeout> | undefined

const copy = async () => {
  await navigator.clipboard.writeText(props.text)
  copied.value = true
  clearTimeout(timer)
  timer = setTimeout(() => (copied.value = false), 2000)
}

onScopeDispose(() => clearTimeout(timer))
</script>

<template>
  <Button
    type="button"
    variant="outline"
    @click="copy().catch(() => {})"
  >
    <Icon :name="copied ? 'lucide:check' : 'lucide:copy'" />
    <span aria-live="polite">{{ copied ? t('secrets.copied') : (label ?? t('secrets.copy')) }}</span>
  </Button>
</template>
