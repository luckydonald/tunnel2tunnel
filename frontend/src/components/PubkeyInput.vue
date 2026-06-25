<script setup lang="ts">
import { ref } from 'vue'

export interface ParsedKey {
  algorithm: string
  key_data: string
  comment: string | null
}

const emit = defineEmits<{
  'update:modelValue': [key: ParsedKey | null]
}>()

const rawText = ref('')
const parseError = ref<string | null>(null)
const fileInput = ref<HTMLInputElement | null>(null)

const VALID_ALGOS = [
  'ssh-rsa', 'ssh-ed25519', 'ecdsa-sha2-nistp256',
  'ecdsa-sha2-nistp384', 'ecdsa-sha2-nistp521', 'sk-ssh-ed25519@openssh.com',
  'sk-ecdsa-sha2-nistp256@openssh.com',
]

function parseAndEmit(text: string): void {
  const line = text.trim()
  if (!line) {
    parseError.value = null
    emit('update:modelValue', null)
    return
  }
  const parts = line.split(/\s+/)
  if (parts.length < 2) {
    parseError.value = 'Expected: algorithm key_data [comment]'
    emit('update:modelValue', null)
    return
  }
  const [algorithm, key_data, ...rest] = parts
  if (!VALID_ALGOS.some(a => algorithm === a)) {
    parseError.value = `Unknown algorithm: ${algorithm}`
    emit('update:modelValue', null)
    return
  }
  parseError.value = null
  emit('update:modelValue', {
    algorithm,
    key_data,
    comment: rest.length ? rest.join(' ') : null,
  })
}

function handleInput(e: Event): void {
  rawText.value = (e.target as HTMLTextAreaElement).value
  parseAndEmit(rawText.value)
}

async function handleFileChange(e: Event): Promise<void> {
  const file = (e.target as HTMLInputElement).files?.[0]
  if (!file) return
  if (file.size > 16 * 1024) {
    parseError.value = 'Key file too large (max 16 KB)'
    return
  }
  const text = await file.text()
  rawText.value = text.trim()
  parseAndEmit(rawText.value)
  ;(e.target as HTMLInputElement).value = ''
}
</script>

<template>
  <div class="pubkey-input">
    <textarea
      :value="rawText"
      placeholder="Paste public key (authorized_keys format)&#10;ssh-ed25519 AAAA... user@host"
      rows="3"
      spellcheck="false"
      autocomplete="off"
      @input="handleInput"
    />
    <div class="file-row">
      <button type="button" class="btn-file" @click="fileInput?.click()">
        Upload .pub file
      </button>
      <input
        ref="fileInput"
        type="file"
        accept=".pub,.pem,.key,text/plain"
        style="display: none"
        @change="handleFileChange"
      />
    </div>
    <p v-if="parseError" class="parse-error">{{ parseError }}</p>
  </div>
</template>

<style lang="scss" scoped>
.pubkey-input {
  display: flex;
  flex-direction: column;
  gap: 0.5rem;
}

textarea {
  width: 100%;
  padding: 0.625rem 0.75rem;
  background: #0f1117;
  border: 1px solid #2d3248;
  border-radius: 4px;
  color: #e2e8f0;
  font-family: 'Fira Code', 'Cascadia Code', monospace;
  font-size: 0.8125rem;
  resize: vertical;
  box-sizing: border-box;

  &:focus {
    outline: none;
    border-color: #4f6ef7;
  }
}

.file-row {
  display: flex;
  align-items: center;
  gap: 0.5rem;
}

.btn-file {
  padding: 0.375rem 0.75rem;
  background: none;
  border: 1px solid #2d3248;
  border-radius: 4px;
  color: #94a3b8;
  font-size: 0.8125rem;
  cursor: pointer;

  &:hover {
    color: #e2e8f0;
    border-color: #4f6ef7;
  }
}

.parse-error {
  margin: 0;
  color: #fca5a5;
  font-size: 0.8125rem;
}
</style>
