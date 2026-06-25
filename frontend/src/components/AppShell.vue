<script setup lang="ts">
import { useRouter, RouterLink } from 'vue-router'
import { useAuthStore } from '@/stores/auth'

const router = useRouter()
const auth = useAuthStore()

async function handleLogout(): Promise<void> {
  await auth.logout()
  await router.push({ name: 'login' })
}
</script>

<template>
  <div class="shell">
    <nav class="sidebar">
      <div class="sidebar-brand">t2t</div>
      <ul class="sidebar-nav">
        <li><RouterLink to="/dashboard">Dashboard</RouterLink></li>
        <li class="nav-group-label">Entities</li>
        <li><RouterLink to="/servers">Servers</RouterLink></li>
        <li><RouterLink to="/clients">Clients</RouterLink></li>
        <li class="nav-group-label">Social</li>
        <li><RouterLink to="/friends">Friends</RouterLink></li>
      </ul>
      <div class="sidebar-footer">
        <span class="username">{{ auth.user?.username }}</span>
        <button class="logout-btn" @click="handleLogout">Logout</button>
      </div>
    </nav>
    <main class="content">
      <slot />
    </main>
  </div>
</template>

<style lang="scss" scoped>
.shell {
  display: flex;
  min-height: 100vh;
  background: #0f1117;
  color: #e2e8f0;
}

.sidebar {
  width: 220px;
  flex-shrink: 0;
  background: #1a1d27;
  border-right: 1px solid #2d3248;
  display: flex;
  flex-direction: column;
  padding: 1rem 0;
}

.sidebar-brand {
  padding: 0 1rem 1rem;
  font-size: 1.25rem;
  font-weight: 700;
  color: #e2e8f0;
  border-bottom: 1px solid #2d3248;
  margin-bottom: 1rem;
  letter-spacing: 0.05em;
}

.sidebar-nav {
  list-style: none;
  margin: 0;
  padding: 0;
  flex: 1;

  li a {
    display: block;
    padding: 0.5rem 1rem;
    color: #94a3b8;
    text-decoration: none;
    font-size: 0.9375rem;

    &:hover,
    &.router-link-active {
      color: #e2e8f0;
      background: #2d3248;
    }
  }
}

.nav-group-label {
  padding: 0.875rem 1rem 0.25rem;
  font-size: 0.6875rem;
  text-transform: uppercase;
  letter-spacing: 0.1em;
  color: #4b5563;
  font-weight: 600;
}

.sidebar-footer {
  padding: 1rem;
  border-top: 1px solid #2d3248;
  display: flex;
  align-items: center;
  gap: 0.5rem;
}

.username {
  flex: 1;
  font-size: 0.8125rem;
  color: #64748b;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.logout-btn {
  background: none;
  border: 1px solid #2d3248;
  border-radius: 4px;
  color: #94a3b8;
  padding: 0.25rem 0.5rem;
  font-size: 0.75rem;
  cursor: pointer;
  white-space: nowrap;

  &:hover {
    color: #e2e8f0;
    border-color: #4f6ef7;
  }
}

.content {
  flex: 1;
  padding: 2rem;
  overflow-y: auto;
}
</style>
