<script setup lang="ts">
import { ref } from 'vue'
import { useRouter } from 'vue-router'
import { useAuthStore } from '@/stores/auth'

const router = useRouter()
const auth = useAuthStore()

const username = ref('')
const password = ref('')
const error = ref<string | null>(null)
const loading = ref(false)

async function handleSubmit(): Promise<void> {
  error.value = null
  loading.value = true
  try {
    await auth.login(username.value, password.value)
    await router.push({ name: 'dashboard' })
  } catch (e) {
    error.value = e instanceof Error ? e.message : 'Login failed'
  } finally {
    loading.value = false
  }
}
</script>

<template>
  <div class="login-page">
    <div class="login-card">
      <h1>tunnel2tunnel</h1>
      <form @submit.prevent="handleSubmit">
        <div class="field">
          <label for="username">Username</label>
          <input
            id="username"
            v-model="username"
            type="text"
            autocomplete="username"
            required
          />
        </div>
        <div class="field">
          <label for="password">Password</label>
          <input
            id="password"
            v-model="password"
            type="password"
            autocomplete="current-password"
            required
          />
        </div>
        <div v-if="error" class="error-msg">{{ error }}</div>
        <button type="submit" :disabled="loading">
          {{ loading ? 'Signing in…' : 'Sign in' }}
        </button>
      </form>
    </div>
  </div>
</template>

<style lang="scss" scoped>
.login-page {
  min-height: 100vh;
  display: flex;
  align-items: center;
  justify-content: center;
  background: #0f1117;
}

.login-card {
  background: #1a1d27;
  border: 1px solid #2d3248;
  border-radius: 8px;
  padding: 2.5rem;
  width: 100%;
  max-width: 380px;

  h1 {
    margin: 0 0 2rem;
    font-size: 1.5rem;
    color: #e2e8f0;
    text-align: center;
  }
}

.field {
  margin-bottom: 1rem;

  label {
    display: block;
    margin-bottom: 0.375rem;
    font-size: 0.875rem;
    color: #94a3b8;
  }

  input {
    width: 100%;
    padding: 0.625rem 0.75rem;
    background: #0f1117;
    border: 1px solid #2d3248;
    border-radius: 4px;
    color: #e2e8f0;
    font-size: 0.9375rem;
    box-sizing: border-box;

    &:focus {
      outline: none;
      border-color: #4f6ef7;
    }
  }
}

.error-msg {
  margin-bottom: 0.75rem;
  padding: 0.5rem 0.75rem;
  background: rgba(239, 68, 68, 0.15);
  border: 1px solid rgba(239, 68, 68, 0.3);
  border-radius: 4px;
  color: #fca5a5;
  font-size: 0.875rem;
}

button[type="submit"] {
  width: 100%;
  padding: 0.625rem;
  background: #4f6ef7;
  border: none;
  border-radius: 4px;
  color: #fff;
  font-size: 0.9375rem;
  cursor: pointer;
  margin-top: 0.5rem;

  &:hover:not(:disabled) {
    background: #3d5ce5;
  }

  &:disabled {
    opacity: 0.6;
    cursor: not-allowed;
  }
}
</style>
