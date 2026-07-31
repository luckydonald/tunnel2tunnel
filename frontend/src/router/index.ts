import { createRouter, createWebHistory, type RouteRecordRaw } from 'vue-router'
import { useAuthStore } from '@/stores/auth'
import LoginPage from '@/pages/LoginPage.vue'
import DashboardPage from '@/pages/DashboardPage.vue'
import EntitiesPage from '@/pages/EntitiesPage.vue'
import EntityDetailPage from '@/pages/EntityDetailPage.vue'
import FriendsPage from '@/pages/FriendsPage.vue'
import AdminUsersPage from '@/pages/AdminUsersPage.vue'
import AdminConnectionLogsPage from '@/pages/AdminConnectionLogsPage.vue'
import AdminBanRulesPage from '@/pages/AdminBanRulesPage.vue'
import AdminLiveConnectionsPage from '@/pages/AdminLiveConnectionsPage.vue'
import SettingsPage from '@/pages/SettingsPage.vue'
import PurgeKeysPage from '@/pages/PurgeKeysPage.vue'
import PurgeAccessPage from '@/pages/PurgeAccessPage.vue'
import SentryTestPage from '@/pages/SentryTestPage.vue'

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
    props: { role: 'server' },
    meta: { requiresAuth: true },
  },
  {
    path: '/clients',
    name: 'clients',
    component: EntitiesPage,
    props: { role: 'client' },
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
  {
    path: '/friends',
    name: 'friends',
    component: FriendsPage,
    meta: { requiresAuth: true },
  },
  {
    path: '/admin/users',
    name: 'admin-users',
    component: AdminUsersPage,
    meta: { requiresAuth: true },
  },
  {
    path: '/admin/connection-logs',
    name: 'admin-connection-logs',
    component: AdminConnectionLogsPage,
    meta: { requiresAuth: true },
  },
  {
    path: '/admin/ban-rules',
    name: 'admin-ban-rules',
    component: AdminBanRulesPage,
    meta: { requiresAuth: true },
  },
  {
    path: '/admin/live-connections',
    name: 'admin-live-connections',
    component: AdminLiveConnectionsPage,
    meta: { requiresAuth: true },
  },
  {
    path: '/settings',
    name: 'settings',
    component: SettingsPage,
    meta: { requiresAuth: true },
  },
  {
    path: '/settings/purge-keys',
    name: 'purge-keys',
    component: PurgeKeysPage,
    meta: { requiresAuth: true },
  },
  {
    path: '/settings/purge-access',
    name: 'purge-access',
    component: PurgeAccessPage,
    meta: { requiresAuth: true },
  },
  {
    // Not linked from nav — visit directly to verify Sentry/Bugsink reporting end to end.
    path: '/sentry-test',
    name: 'sentry-test',
    component: SentryTestPage,
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
