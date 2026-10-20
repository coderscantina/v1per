<script setup lang="ts">
// Adapted from Kit (f447e797, MIT): resources/js/components/app/AppUserMenu.vue. Laravel auth and
// abilities are replaced by the `auth.session` resource; the PWA install entry is dropped. Account
// links lead to security and API keys. Appearance, language and formats are set on the account's
// Appearance page.
import Icon from '~/components/Icon.vue'
import { Avatar } from '~/components/ui/avatar'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuShortcut,
  DropdownMenuTrigger,
} from '~/components/ui/dropdown-menu'
import { useSession, useSignOut } from '~/composables/useSession'
import { useShortcutHelp } from '~/composables/useShortcuts'
import { initials } from '~/lib/utils'
import { useI18n } from '~/plugins/i18n'

const { t } = useI18n()
const { account } = useSession()
const signOut = useSignOut()
const shortcutHelp = useShortcutHelp()
</script>

<template>
  <DropdownMenu>
    <DropdownMenuTrigger
      class="grid size-shell-control shrink-0 cursor-pointer place-items-center rounded-full bg-fill transition-colors duration-200 ease-butter hover:bg-fill-strong data-[state=open]:bg-fill-strong"
      :aria-label="t('shell.userMenu', { initials: initials(account?.name ?? '?') })"
    >
      <Avatar
        class="size-full rounded-full bg-transparent text-secondary-foreground"
        :name="account?.name ?? '?'"
      />
    </DropdownMenuTrigger>

    <DropdownMenuContent
      align="end"
      class="w-64"
    >
      <div class="flex items-center gap-3 px-2 py-2">
        <Avatar
          :name="account?.name ?? '?'"
          class="shrink-0 rounded-lg"
        />
        <div class="min-w-0">
          <p class="truncate text-sm font-semibold text-primary">{{ account?.name }}</p>
          <p class="truncate text-xs text-muted">{{ account?.email }}</p>
        </div>
      </div>
      <DropdownMenuSeparator />
      <DropdownMenuItem as-child>
        <RouterLink :to="{ name: 'account-security' }">
          <Icon name="lucide:shield" />
          {{ t('account.nav.security') }}
        </RouterLink>
      </DropdownMenuItem>
      <DropdownMenuItem as-child>
        <RouterLink :to="{ name: 'account-appearance' }">
          <Icon name="lucide:palette" />
          {{ t('account.nav.appearance') }}
        </RouterLink>
      </DropdownMenuItem>
      <DropdownMenuItem as-child>
        <RouterLink :to="{ name: 'account-api-keys' }">
          <Icon name="lucide:key-round" />
          {{ t('account.nav.apiKeys') }}
        </RouterLink>
      </DropdownMenuItem>
      <DropdownMenuSeparator />


      <DropdownMenuItem
        class="max-lg:hidden"
        @select="shortcutHelp.show()"
      >
        <Icon name="lucide:keyboard" />
        {{ t('shortcuts.title') }}
        <DropdownMenuShortcut>?</DropdownMenuShortcut>
      </DropdownMenuItem>

      <DropdownMenuSeparator />
      <DropdownMenuItem @select="signOut()">
        <Icon name="lucide:log-out" />
        {{ t('auth.signOut') }}
      </DropdownMenuItem>
    </DropdownMenuContent>
  </DropdownMenu>
</template>
