<script setup lang="ts">
import { useMutation } from '@tanstack/vue-query'

import { pagesResource } from '~/api/resources/pages'
import CommandAlert from '~/components/CommandAlert.vue'
import FormField from '~/components/FormField.vue'
import { Button } from '~/components/ui/button'
import { Dialog, DialogContent, DialogHeaderCombined } from '~/components/ui/dialog'
import { useErrorMessage } from '~/composables/useErrorMessage'
import type { NodeId } from '~/generated'
import { PAGE_PATH } from '~/generated/limits'
import { slugify } from '~/lib/pages'
import { useI18n } from '~/plugins/i18n'

/**
 * Creates a status page on the node from a title and a slug, then opens its editor. The slug
 * follows the title until edited. New pages start restricted to the node's team.
 */
const props = defineProps<{ node: NodeId }>()
const open = defineModel<boolean>('open', { default: false })

const { t } = useI18n()
const { fieldMessage } = useErrorMessage()
const router = useRouter()
const title = ref('')
const slug = ref('')
const slugEdited = ref(false)

watch(title, (value) => {
  if (!slugEdited.value) slug.value = slugify(value)
})
const editSlug = (value: string) => {
  slug.value = value
  slugEdited.value = true
}

const create = useMutation({
  mutationFn: () => pagesResource.create({ node: props.node, title: title.value.trim(), slug: slug.value.trim() }),
  onSuccess: ({ page }) => {
    open.value = false
    void router.push({ name: 'node-page-layout', params: { nodeId: page.node, pageId: page.id } })
  },
})

watch(open, (value) => {
  if (!value) return
  title.value = ''
  slug.value = ''
  slugEdited.value = false
  create.reset()
})
</script>

<template>
  <Dialog v-model:open="open">
    <DialogContent class="max-w-lg">
      <DialogHeaderCombined
        :title="t('pages.create')"
        :description="t('pages.createDescription')"
      />
      <form
        class="grid gap-4"
        novalidate
        @submit.prevent="create.mutate()"
      >
        <FormField
          id="page-create-title"
          v-model="title"
          autocomplete="off"
          :label="t('pages.settings.title')"
          :error="fieldMessage(create.error.value, 'title')"
          required
        />
        <FormField
          id="page-create-slug"
          :model-value="slug"
          autocomplete="off"
          :label="t('pages.settings.slug')"
          :description="t('pages.settings.slugHint', { path: `${PAGE_PATH}${slug || 'status'}` })"
          :error="fieldMessage(create.error.value, 'slug')"
          required
          @update:model-value="editSlug"
        />
        <CommandAlert
          :error="create.error.value"
          :shown="['title', 'slug']"
        />
        <div class="flex justify-end gap-2">
          <Button
            type="button"
            variant="ghost"
            @click="open = false"
          >
            {{ t('actions.cancel') }}
          </Button>
          <Button
            type="submit"
            variant="primary"
            :loading="create.isPending.value"
            :disabled="title.trim() === '' || slug.trim() === ''"
          >
            {{ t('pages.create') }}
          </Button>
        </div>
      </form>
    </DialogContent>
  </Dialog>
</template>
