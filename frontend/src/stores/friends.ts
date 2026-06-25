import { ref } from 'vue'
import { defineStore } from 'pinia'
import { friendsApi, type Friendship } from '@/api/friends'

export const useFriendsStore = defineStore('friends', () => {
  const friendships = ref<Friendship[]>([])
  const loading = ref(false)
  const error = ref<string | null>(null)

  async function fetchFriends(): Promise<void> {
    loading.value = true
    error.value = null
    try {
      friendships.value = await friendsApi.listFriends()
    } catch (e) {
      error.value = e instanceof Error ? e.message : 'Failed to load friends'
    } finally {
      loading.value = false
    }
  }

  async function sendRequest(username: string): Promise<Friendship> {
    const f = await friendsApi.sendRequest(username)
    friendships.value.unshift(f)
    return f
  }

  async function updateFriendship(
    id: string,
    data: { status?: string; visibility_grant?: string },
  ): Promise<void> {
    const updated = await friendsApi.updateFriendship(id, data)
    const idx = friendships.value.findIndex(f => f.id === id)
    if (idx >= 0) friendships.value[idx] = updated
  }

  return { friendships, loading, error, fetchFriends, sendRequest, updateFriendship }
})
