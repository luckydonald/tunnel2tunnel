import { ref } from 'vue'
import { defineStore } from 'pinia'
import { entitiesApi, type Entity, type CreateEntityParams } from '@/api/entities'

export const useEntitiesStore = defineStore('entities', () => {
  const entities = ref<Entity[]>([])
  const loading = ref(false)
  const error = ref<string | null>(null)

  // Kept separate from `entities` (which reflects whatever role filter is
  // currently on-page) — this is a standalone count for the sidebar's
  // Unassigned badge, refreshed independently of the visible list.
  const unassignedCount = ref<number | null>(null)

  async function fetchEntities(role?: 'server' | 'client' | 'unassigned'): Promise<void> {
    loading.value = true
    error.value = null
    try {
      entities.value = await entitiesApi.list(role)
    } catch (e) {
      error.value = e instanceof Error ? e.message : 'Failed to load entities'
    } finally {
      loading.value = false
    }
  }

  async function fetchUnassignedCount(): Promise<void> {
    try {
      unassignedCount.value = (await entitiesApi.list('unassigned')).length
    } catch {
      // non-critical; badge just stays at its last known value
    }
  }

  async function createEntity(params: CreateEntityParams): Promise<Entity> {
    const entity = await entitiesApi.create(params)
    entities.value.unshift(entity)
    return entity
  }

  async function deleteEntity(id: string): Promise<void> {
    await entitiesApi.deleteEntity(id)
    entities.value = entities.value.filter(e => e.id !== id)
  }

  return { entities, loading, error, unassignedCount, fetchEntities, fetchUnassignedCount, createEntity, deleteEntity }
})
