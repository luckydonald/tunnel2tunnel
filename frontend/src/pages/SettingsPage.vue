<script setup lang="ts">
import { ref } from 'vue'
import { useRouter } from 'vue-router'
import AppShell from '@/components/AppShell.vue'
import { adminApi } from '@/api/admin'

const router = useRouter()
const oldPw = ref('')
const newPw = ref('')
const confirmPw = ref('')
const pwError = ref<string | null>(null)
const pwSuccess = ref(false)
const saving = ref(false)

async function handleChangePassword(): Promise<void> {
  pwError.value = null
  pwSuccess.value = false
  if (newPw.value !== confirmPw.value) {
    pwError.value = 'Passwords do not match'
    return
  }
  if (newPw.value.length < 8) {
    pwError.value = 'Password must be at least 8 characters'
    return
  }
  saving.value = true
  try {
    await adminApi.changePassword(oldPw.value, newPw.value)
    pwSuccess.value = true
    oldPw.value = ''
    newPw.value = ''
    confirmPw.value = ''
  } catch (e) {
    pwError.value = e instanceof Error ? e.message : 'Failed'
  } finally {
    saving.value = false
  }
}
</script>

<template>
  <AppShell>
    <h1 class="page-title">Settings</h1>

    <!-- Change password -->
    <section class="section">
      <h2>Change Password</h2>
      <div class="card">
        <div class="field">
          <label>Current password</label>
          <input v-model="oldPw" type="password" autocomplete="current-password" />
        </div>
        <div class="field">
          <label>New password</label>
          <input v-model="newPw" type="password" autocomplete="new-password" />
        </div>
        <div class="field">
          <label>Confirm new password</label>
          <input v-model="confirmPw" type="password" autocomplete="new-password" />
        </div>
        <p v-if="pwError" class="error-msg">{{ pwError }}</p>
        <p v-if="pwSuccess" class="success-msg">Password updated.</p>
        <button
          class="btn-primary"
          :disabled="saving || !oldPw || !newPw || !confirmPw"
          @click="handleChangePassword"
        >
          {{ saving ? 'Saving…' : 'Update password' }}
        </button>
      </div>
    </section>

    <!-- Purge links -->
    <section class="section">
      <h2>Data Management</h2>
      <div class="card link-list">
        <RouterLink to="/settings/purge-keys" class="purge-link">
          <span class="link-title">Purge SSH Keys</span>
          <span class="link-desc">Revoke SSH keys associated with your entities</span>
        </RouterLink>
        <RouterLink to="/settings/purge-access" class="purge-link">
          <span class="link-title">Purge Access Rules</span>
          <span class="link-desc">Remove entity access rules you have created</span>
        </RouterLink>
      </div>
    </section>
  </AppShell>
</template>

<style lang="scss" scoped>
.page-title { margin-bottom: 2rem; }

.section {
  margin-bottom: 2.5rem;
  h2 { font-size: 1rem; color: #94a3b8; margin-bottom: 1rem; text-transform: uppercase; letter-spacing: 0.06em; }
}

.card {
  background: #1a1d27; border: 1px solid #2d3248; border-radius: 8px; padding: 1.5rem;
  max-width: 480px;
}

.field {
  margin-bottom: 1rem;
  label { display: block; margin-bottom: 0.375rem; font-size: 0.875rem; color: #94a3b8; }
  input {
    width: 100%; padding: 0.5rem 0.75rem; background: #0f1117;
    border: 1px solid #2d3248; border-radius: 4px; color: #e2e8f0;
    font-size: 0.9375rem; box-sizing: border-box;
    &:focus { outline: none; border-color: #4f6ef7; }
  }
}

.link-list { display: flex; flex-direction: column; gap: 0; }

.purge-link {
  display: flex; flex-direction: column; padding: 0.875rem 0;
  border-bottom: 1px solid #2d3248; text-decoration: none;
  &:last-child { border-bottom: none; }
  &:hover .link-title { color: #7dd3fc; }
}

.link-title { color: #e2e8f0; font-size: 0.9375rem; margin-bottom: 0.125rem; }
.link-desc { color: #64748b; font-size: 0.8125rem; }

.btn-primary {
  padding: 0.5rem 1.25rem; background: #4f6ef7; border: none; border-radius: 4px;
  color: #fff; font-size: 0.9375rem; cursor: pointer; margin-top: 0.5rem;
  &:hover:not(:disabled) { background: #3d5ce5; }
  &:disabled { opacity: 0.6; cursor: not-allowed; }
}

.error-msg { color: #fca5a5; margin-bottom: 0.75rem; }
.success-msg { color: #6ee7b7; margin-bottom: 0.75rem; }
</style>
