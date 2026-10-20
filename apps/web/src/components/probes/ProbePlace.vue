<script setup lang="ts">
import type { ProbeLocation } from '~/generated'
import { countryName, flagEmoji } from '~/lib/countries'
import { useI18n } from '~/plugins/i18n'

/**
 * Where a probe runs in one line: the country's flag, then the city (or the country's name
 * without one) and the provider. Renders nothing for a probe without a location.
 */
const props = defineProps<{ location: ProbeLocation }>()

const { locale } = useI18n()

const country = computed(() => (props.location.country ? countryName(props.location.country, locale.value) : null))
const parts = computed(() =>
  [props.location.city ?? country.value, props.location.provider].filter((part): part is string => !!part)
)
</script>

<template>
  <span
    v-if="location.country || parts.length > 0"
    class="inline-flex min-w-0 items-center gap-1.5"
  >
    <span
      v-if="location.country"
      class="shrink-0 text-[1.05em] leading-none"
      role="img"
      :aria-label="country ?? location.country"
      :title="country ?? location.country"
    >{{ flagEmoji(location.country) }}</span>
    <span class="min-w-0 truncate">{{ parts.join(' · ') }}</span>
  </span>
</template>
