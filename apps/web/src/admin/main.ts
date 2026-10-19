import { createApp } from 'vue'

import { appConfig } from '~/lib/app-config'
import { installAuthHandler } from '~/plugins/auth'
import { installI18n } from '~/plugins/i18n'
import { installVueQuery } from '~/plugins/vue-query'
import { loadRuntimeConfig } from '~/services/runtime-config'

import App from './App.vue'
import { router } from './router'

import '~/assets/css/app.css'

document.documentElement.dataset.density = appConfig.density

await loadRuntimeConfig()

const app = createApp(App)
installI18n(app)
installVueQuery(app)
installAuthHandler(router)
app.use(router)
app.mount('#app')
