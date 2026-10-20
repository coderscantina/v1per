<script setup lang="ts">
import FormField from '~/components/FormField.vue'
import SelectField from '~/components/SelectField.vue'
import type { ProbeLocation } from '~/generated'
import { NAME_MAX_CHARS } from '~/generated/limits'
import { countryCodes, countryName, flagEmoji } from '~/lib/countries'
import { useI18n } from '~/plugins/i18n'

/** Country, city and provider of a probe, for the create and edit dialogs. */
defineProps<{
  /** Prefix for the field IDs. */
  id: string
  /** A field's error by its name, such as `city`. */
  error: (field: keyof ProbeLocation) => string | null | undefined
}>()

const location = defineModel<ProbeLocation>({ required: true })

const { t, locale } = useI18n()

/** Select items cannot carry an empty value, so no country is this one. */
const NONE = '-'
const countries = computed(() => [
  { value: NONE, label: t('probes.noCountry') },
  ...countryCodes
    .map((code) => ({ code, name: countryName(code, locale.value) }))
    .sort((a, b) => a.name.localeCompare(b.name, locale.value))
    .map(({ code, name }) => ({ value: code, label: `${flagEmoji(code)}  ${name}` })),
])
const country = computed({
  get: () => location.value.country ?? NONE,
  set: (code: string) => (location.value = { ...location.value, country: code === NONE ? undefined : code }),
})
const text = (field: 'city' | 'provider') =>
  computed({
    get: () => location.value[field] ?? '',
    set: (value: string) => (location.value = { ...location.value, [field]: value }),
  })
const city = text('city')
const provider = text('provider')
</script>

<template>
  <div class="grid gap-4 sm:grid-cols-2">
    <SelectField
      :id="`${id}-country`"
      v-model="country"
      class="sm:col-span-2"
      :label="t('probes.country')"
      :options="countries"
      :error="error('country')"
    />
    <FormField
      :id="`${id}-city`"
      v-model="city"
      autocomplete="off"
      :label="t('probes.city')"
      :placeholder="t('probes.cityPlaceholder')"
      :maxlength="NAME_MAX_CHARS"
      :error="error('city')"
    />
    <FormField
      :id="`${id}-provider`"
      v-model="provider"
      autocomplete="off"
      :label="t('probes.provider')"
      :placeholder="t('probes.providerPlaceholder')"
      :maxlength="NAME_MAX_CHARS"
      :error="error('provider')"
    />
  </div>
</template>
