import { createApp } from 'vue'
import { createPinia } from 'pinia'
import App from './App.vue'
import router from './router'
import { initSentry } from './sentry'
import './style/main.scss'

const app = createApp(App)
initSentry(app, router)
app.use(createPinia())
app.use(router)
app.mount('#app')
