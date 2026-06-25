import { ref } from 'vue'
import { defineStore } from 'pinia'
import { authApi, type User } from '@/api/auth'

export const useAuthStore = defineStore('auth', () => {
  const user = ref<User | null>(null)

  async function fetchMe(): Promise<void> {
    user.value = await authApi.me()
  }

  async function login(username: string, password: string): Promise<void> {
    user.value = await authApi.login(username, password)
  }

  async function logout(): Promise<void> {
    await authApi.logout()
    user.value = null
  }

  return { user, fetchMe, login, logout }
})
