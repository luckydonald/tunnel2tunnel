import { createRouter, createWebHistory, type RouteRecordRaw } from 'vue-router'
import { useAuthStore } from '@/stores/auth'
import LoginPage from '@/pages/LoginPage.vue'
import DashboardPage from '@/pages/DashboardPage.vue'
import EntitiesPage from '@/pages/EntitiesPage.vue'
import EntityDetailPage from '@/pages/EntityDetailPage.vue'

const routes: RouteRecordRaw[] = [
  { path: '/login', name: 'login', component: LoginPage },
  { path: '/', redirect: '/dashboard' },
  {
    path: '/dashboard',
    name: 'dashboard',
    component: DashboardPage,
    meta: { requiresAuth: true },
  },
  {
    path: '/servers',
    name: 'servers',
    component: EntitiesPage,
    props: { entityType: 'server' },
    meta: { requiresAuth: true },
  },
  {
    path: '/clients',
    name: 'clients',
    component: EntitiesPage,
    props: { entityType: 'client' },
    meta: { requiresAuth: true },
  },
  {
    path: '/entities',
    name: 'entities',
    component: EntitiesPage,
    meta: { requiresAuth: true },
  },
  {
    path: '/entities/:id',
    name: 'entity-detail',
    component: EntityDetailPage,
    meta: { requiresAuth: true },
  },
]

const router = createRouter({
  history: createWebHistory(),
  routes,
})

router.beforeEach(async (to) => {
  const auth = useAuthStore()

  if (to.meta.requiresAuth === true) {
    if (!auth.user) {
      try { await auth.fetchMe() } catch { /* not logged in */ }
    }
    if (!auth.user) return { name: 'login' }
  }

  if (to.name === 'login' && auth.user) return { name: 'dashboard' }
  return true
})

export default router
