<script setup lang="ts">
import { ref, onMounted } from 'vue'
import AppShell from '@/components/AppShell.vue'
import { useAuthStore } from '@/stores/auth'
import { adminApi, type AdminUser } from '@/api/admin'

const auth = useAuthStore()
const users = ref<AdminUser[]>([])
const loading = ref(true)
const error = ref<string | null>(null)

// Edit modal state
const editing = ref<AdminUser | null>(null)
const editForm = ref({ email: '', is_admin: false, is_locked: false, description: '', password: '' })
const editError = ref<string | null>(null)
const saving = ref(false)

// Create modal state
const showCreate = ref(false)
const createForm = ref({ username: '', email: '', password: '', is_admin: false, description: '' })
const createError = ref<string | null>(null)
const creating = ref(false)

onMounted(async () => {
  try {
    users.value = await adminApi.listUsers()
  } catch (e) {
    error.value = e instanceof Error ? e.message : 'Failed to load users'
  } finally {
    loading.value = false
  }
})

function openEdit(u: AdminUser): void {
  editing.value = u
  editForm.value = {
    email: u.email ?? '',
    is_admin: u.is_admin,
    is_locked: u.is_locked,
    description: u.description ?? '',
    password: '',
  }
  editError.value = null
}

async function handleSaveEdit(): Promise<void> {
  if (!editing.value) return
  saving.value = true
  editError.value = null
  try {
    const updated = await adminApi.updateUser(editing.value.id, {
      email: editForm.value.email || null,
      is_admin: editForm.value.is_admin,
      is_locked: editForm.value.is_locked,
      description: editForm.value.description || null,
      password: editForm.value.password || undefined,
    })
    const idx = users.value.findIndex(u => u.id === updated.id)
    if (idx >= 0) users.value[idx] = updated
    editing.value = null
  } catch (e) {
    editError.value = e instanceof Error ? e.message : 'Failed to save'
  } finally {
    saving.value = false
  }
}

async function handleCreate(): Promise<void> {
  creating.value = true
  createError.value = null
  try {
    const u = await adminApi.createUser({
      username: createForm.value.username,
      email: createForm.value.email || null,
      password: createForm.value.password,
      is_admin: createForm.value.is_admin,
      description: createForm.value.description || null,
    })
    users.value.push(u)
    showCreate.value = false
    createForm.value = { username: '', email: '', password: '', is_admin: false, description: '' }
  } catch (e) {
    createError.value = e instanceof Error ? e.message : 'Failed to create'
  } finally {
    creating.value = false
  }
}

function isSelf(u: AdminUser): boolean {
  return u.id === auth.user?.id
}
</script>

<template>
  <AppShell>
    <div class="page-header">
      <h1>Users</h1>
      <button class="btn-primary" @click="showCreate = true">New User</button>
    </div>

    <div v-if="loading" class="loading">Loading…</div>
    <div v-else-if="error" class="error-msg">{{ error }}</div>
    <template v-else>
      <table class="data-table">
        <thead>
          <tr>
            <th>Username</th>
            <th>Email</th>
            <th>Admin</th>
            <th>Locked</th>
            <th>Description</th>
            <th></th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="u in users" :key="u.id" :id="`user-${u.id}`" :class="{ 'row-self': isSelf(u) }">
            <td>
              {{ u.username }}
              <span v-if="isSelf(u)" class="badge-you">you</span>
            </td>
            <td class="td-sec">{{ u.email ?? '—' }}</td>
            <td><span v-if="u.is_admin" class="badge-admin">admin</span><span v-else>—</span></td>
            <td><span v-if="u.is_locked" class="badge-locked">locked</span><span v-else>—</span></td>
            <td class="td-sec">{{ u.description ?? '—' }}</td>
            <td>
              <button class="btn-edit" @click="openEdit(u)">Edit</button>
            </td>
          </tr>
        </tbody>
      </table>
    </template>

    <!-- Edit modal -->
    <div v-if="editing" class="modal-overlay" @click.self="editing = null">
      <div class="modal">
        <h2>Edit {{ editing.username }}</h2>
        <div class="field">
          <label>Email</label>
          <input v-model="editForm.email" type="email" placeholder="user@example.com" />
        </div>
        <div class="field">
          <label>Description</label>
          <input v-model="editForm.description" type="text" />
        </div>
        <div class="field">
          <label>New password <span class="optional">(leave blank to keep)</span></label>
          <input v-model="editForm.password" type="password" autocomplete="new-password" />
        </div>
        <div class="field-row">
          <label class="checkbox-label">
            <input
              v-model="editForm.is_admin"
              type="checkbox"
              :disabled="isSelf(editing) && editing.is_admin"
            />
            Admin
            <span v-if="isSelf(editing) && editing.is_admin" class="hint">(cannot demote self)</span>
          </label>
          <label class="checkbox-label">
            <input v-model="editForm.is_locked" type="checkbox" :disabled="isSelf(editing)" />
            Locked
          </label>
        </div>
        <p v-if="editError" class="error-msg">{{ editError }}</p>
        <div class="modal-actions">
          <button class="btn-secondary" @click="editing = null">Cancel</button>
          <button class="btn-primary" :disabled="saving" @click="handleSaveEdit">
            {{ saving ? 'Saving…' : 'Save' }}
          </button>
        </div>
      </div>
    </div>

    <!-- Create modal -->
    <div v-if="showCreate" class="modal-overlay" @click.self="showCreate = false">
      <div class="modal">
        <h2>New User</h2>
        <div class="field">
          <label>Username</label>
          <input v-model="createForm.username" type="text" autocomplete="off" />
        </div>
        <div class="field">
          <label>Email <span class="optional">(optional)</span></label>
          <input v-model="createForm.email" type="email" />
        </div>
        <div class="field">
          <label>Password</label>
          <input v-model="createForm.password" type="password" autocomplete="new-password" />
        </div>
        <div class="field">
          <label>Description <span class="optional">(optional)</span></label>
          <input v-model="createForm.description" type="text" />
        </div>
        <div class="field-row">
          <label class="checkbox-label">
            <input v-model="createForm.is_admin" type="checkbox" /> Admin
          </label>
        </div>
        <p v-if="createError" class="error-msg">{{ createError }}</p>
        <div class="modal-actions">
          <button class="btn-secondary" @click="showCreate = false">Cancel</button>
          <button
            class="btn-primary"
            :disabled="creating || !createForm.username || !createForm.password"
            @click="handleCreate"
          >
            {{ creating ? 'Creating…' : 'Create' }}
          </button>
        </div>
      </div>
    </div>
  </AppShell>
</template>

<style lang="scss" scoped>
.page-header {
  display: flex; align-items: center; justify-content: space-between; margin-bottom: 1.5rem;
  h1 { margin: 0; }
}

.data-table {
  width: 100%; border-collapse: collapse; font-size: 0.9375rem;
  th {
    text-align: left; padding: 0.5rem 0.75rem; border-bottom: 1px solid #2d3248;
    color: #64748b; font-size: 0.75rem; text-transform: uppercase; letter-spacing: 0.06em;
  }
  td { padding: 0.625rem 0.75rem; border-bottom: 1px solid #1e2235; color: #e2e8f0; }
}

.row-self td { background: rgba(79,110,247,.06); }

.td-sec { color: #94a3b8; }

.badge-you {
  display: inline-block; padding: 0.1em 0.4em; border-radius: 3px;
  background: rgba(79,110,247,.2); color: #93c5fd; font-size: 0.6875rem;
  font-weight: 600; margin-left: 0.375rem; text-transform: uppercase;
}

.badge-admin {
  display: inline-block; padding: 0.1em 0.4em; border-radius: 3px;
  background: rgba(251,191,36,.15); color: #fcd34d; font-size: 0.75rem; font-weight: 600;
}

.badge-locked {
  display: inline-block; padding: 0.1em 0.4em; border-radius: 3px;
  background: rgba(239,68,68,.15); color: #fca5a5; font-size: 0.75rem; font-weight: 600;
}

.btn-edit {
  padding: 0.25rem 0.625rem; background: none; border: 1px solid #2d3248;
  border-radius: 4px; color: #94a3b8; font-size: 0.75rem; cursor: pointer;
  &:hover { color: #e2e8f0; border-color: #4f6ef7; }
}

.modal-overlay {
  position: fixed; inset: 0; background: rgba(0,0,0,.6);
  display: flex; align-items: center; justify-content: center; z-index: 100;
}

.modal {
  background: #1a1d27; border: 1px solid #2d3248; border-radius: 8px;
  padding: 2rem; width: 100%; max-width: 440px;
  h2 { margin: 0 0 1.5rem; font-size: 1.125rem; }
}

.field {
  margin-bottom: 1rem;
  label { display: block; margin-bottom: 0.375rem; font-size: 0.875rem; color: #94a3b8; }
  .optional { color: #64748b; }
  input {
    width: 100%; padding: 0.5rem 0.75rem; background: #0f1117;
    border: 1px solid #2d3248; border-radius: 4px; color: #e2e8f0;
    font-size: 0.9375rem; box-sizing: border-box;
    &:focus { outline: none; border-color: #4f6ef7; }
    &:disabled { opacity: 0.5; }
  }
}

.field-row {
  display: flex; gap: 1.5rem; margin-bottom: 1rem;
}

.checkbox-label {
  display: flex; align-items: center; gap: 0.375rem; font-size: 0.875rem; color: #94a3b8;
  cursor: pointer;
  input { width: auto; }
}

.hint { font-size: 0.75rem; color: #64748b; margin-left: 0.25rem; }

.modal-actions {
  display: flex; gap: 0.75rem; justify-content: flex-end; margin-top: 1.5rem;
}

.btn-primary {
  padding: 0.5rem 1.25rem; background: #4f6ef7; border: none; border-radius: 4px;
  color: #fff; font-size: 0.9375rem; cursor: pointer;
  &:hover:not(:disabled) { background: #3d5ce5; }
  &:disabled { opacity: 0.6; cursor: not-allowed; }
}

.btn-secondary {
  padding: 0.5rem 1.25rem; background: none; border: 1px solid #2d3248; border-radius: 4px;
  color: #94a3b8; font-size: 0.9375rem; cursor: pointer;
  &:hover { color: #e2e8f0; border-color: #4f6ef7; }
}

.loading { color: #94a3b8; }
.error-msg { color: #fca5a5; }
</style>
