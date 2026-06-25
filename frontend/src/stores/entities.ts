import { ref } from 'vue'
import { defineStore } from 'pinia'
import { entitiesApi, type Entity, type CreateEntityParams } from '@/api/entities'

export const useEntitiesStore = defineStore('entities', () => {
  const entities = ref<Entity[]>([])
  const loading = ref(false)
  const error = ref<string | null>(null)

  async function fetchEntities(entityType?: 'server' | 'client'): Promise<void> {
    loading.value = true
    error.value = null
    try {
      entities.value = await entitiesApi.list(entityType)
    } catch (e) {
      error.value = e instanceof Error ? e.message : 'Failed to load entities'
    } finally {
      loading.value = false
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

  return { entities, loading, error, fetchEntities, createEntity, deleteEntity }
})
