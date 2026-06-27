<script setup lang="ts">
import { ref, computed } from 'vue'
import { generateEd25519KeyPair, downloadText } from '@/crypto'

export interface ParsedKey {
  algorithm: string
  key_data: string
  comment: string | null
}

const props = defineProps<{
  filename: string
}>()

const emit = defineEmits<{
  'update:modelValue': [key: ParsedKey | null]
  'update:filename':   [name: string]
}>()

defineOptions({ inheritAttrs: false })

const rawText          = ref('')
const parseError       = ref<string | null>(null)
const fileInput        = ref<HTMLInputElement | null>(null)
const showPrivKeyWarn  = ref(false)
const generating       = ref(false)
const generateError    = ref<string | null>(null)

const VALID_ALGOS = [
  'ssh-rsa', 'ssh-ed25519', 'ecdsa-sha2-nistp256',
  'ecdsa-sha2-nistp384', 'ecdsa-sha2-nistp521', 'sk-ssh-ed25519@openssh.com',
  'sk-ecdsa-sha2-nistp256@openssh.com',
]

const GEN_CMD = computed(() =>
  `ssh-keygen -t ed25519 -C "${props.filename}" -f ~/.ssh/${props.filename}`
)

const copied = ref(false)
async function copyCmd(): Promise<void> {
  await navigator.clipboard.writeText(GEN_CMD.value)
  copied.value = true
  setTimeout(() => { copied.value = false }, 2000)
}

function detectAndHandlePrivateKey(text: string): boolean {
  if (text.includes('-----BEGIN') && text.includes('PRIVATE KEY-----')) {
    // Redact body — keep only header and footer
    const headerMatch = text.match(/(-----BEGIN [^-]+ PRIVATE KEY-----)/s)
    const footerMatch = text.match(/(-----END [^-]+ PRIVATE KEY-----)/s)
    const header = headerMatch?.[1] ?? '-----BEGIN OPENSSH PRIVATE KEY-----'
    const footer = footerMatch?.[1] ?? '-----END OPENSSH PRIVATE KEY-----'
    rawText.value = `${header}\n…\n${footer}`
    showPrivKeyWarn.value = true
    parseError.value = null
    emit('update:modelValue', null)
    return true
  }
  return false
}

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
  emit('update:modelValue', { algorithm, key_data, comment: rest.length ? rest.join(' ') : null })
}

function handleInput(e: Event): void {
  const text = (e.target as HTMLTextAreaElement).value
  rawText.value = text
  if (!detectAndHandlePrivateKey(text)) parseAndEmit(text)
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
  if (!detectAndHandlePrivateKey(text.trim())) parseAndEmit(text.trim())
  ;(e.target as HTMLInputElement).value = ''
}

async function handleGenerate(): Promise<void> {
  generating.value = true
  generateError.value = null
  try {
    const { privateKeyPem, publicKeyLine } = await generateEd25519KeyPair(props.filename)
    downloadText(props.filename, privateKeyPem)
    rawText.value = publicKeyLine
    parseAndEmit(publicKeyLine)
  } catch (e) {
    generateError.value = e instanceof Error ? e.message : 'Key generation failed'
  } finally {
    generating.value = false
  }
}
</script>

<template>
  <div class="pubkey-input">
    <!-- Private key warning overlay -->
    <div v-if="showPrivKeyWarn" class="privkey-overlay">
      <div class="privkey-dialog">
        <div class="privkey-icon">⚠</div>
        <h3>Private key detected</h3>
        <p>
          You pasted a <strong>private key</strong>. Never share your private key with anyone —
          only the public key (<code>.pub</code> file) should be uploaded here.
        </p>
        <p class="privkey-hint">The key content has been removed from the field.</p>
        <button class="btn-ok" @click="showPrivKeyWarn = false; rawText = ''">Got it</button>
      </div>
    </div>

    <!-- Filename field -->
    <div class="filename-row">
      <label class="filename-label">Key filename</label>
      <input
        class="filename-input"
        type="text"
        :value="filename"
        spellcheck="false"
        @input="emit('update:filename', ($event.target as HTMLInputElement).value)"
      />
    </div>

    <!-- Generate hint -->
    <div class="gen-hint">
      <span class="gen-label">Generate a key:</span>
      <code class="gen-cmd">{{ GEN_CMD }}</code>
      <button type="button" class="btn-copy" :class="{ copied }" @click="copyCmd">
        {{ copied ? 'Copied!' : 'Copy' }}
      </button>
    </div>
    <p class="gen-note">
      Then paste the contents of <code>~/.ssh/{{ filename }}.pub</code> below,
      or use <strong>Generate in browser</strong> to create a key offline.
    </p>

    <!-- Public key textarea -->
    <textarea
      :value="rawText"
      placeholder="Paste public key (authorized_keys format)&#10;ssh-ed25519 AAAA... user@host"
      rows="3"
      spellcheck="false"
      autocomplete="off"
      @input="handleInput"
    />

    <!-- Actions row -->
    <div class="actions-row">
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
      <button
        type="button"
        class="btn-generate"
        :disabled="generating"
        @click="handleGenerate"
      >
        {{ generating ? 'Generating…' : 'Generate in browser' }}
      </button>
    </div>

    <p v-if="parseError"    class="msg-error">{{ parseError }}</p>
    <p v-if="generateError" class="msg-error">{{ generateError }}</p>
  </div>
</template>

<style lang="scss" scoped>
.pubkey-input {
  display: flex;
  flex-direction: column;
  gap: 0.5rem;
  position: relative;
}

/* ── Private key overlay ── */
.privkey-overlay {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.7);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 200;
}

.privkey-dialog {
  background: #1a1d27;
  border: 1px solid #ef4444;
  border-radius: 10px;
  padding: 2rem;
  max-width: 420px;
  width: 90%;
  text-align: center;

  h3 { margin: 0.5rem 0 1rem; color: #fca5a5; font-size: 1.1rem; }
  p  { color: #94a3b8; font-size: 0.9rem; line-height: 1.5; margin: 0 0 0.75rem; }

  code {
    background: #0f1117;
    padding: 0.1em 0.3em;
    border-radius: 3px;
    color: #e2e8f0;
  }
}

.privkey-icon {
  font-size: 2rem;
  color: #f87171;
}

.privkey-hint {
  font-size: 0.8125rem !important;
  color: #64748b !important;
}

.btn-ok {
  margin-top: 0.5rem;
  padding: 0.5rem 1.5rem;
  background: rgba(239, 68, 68, 0.15);
  border: 1px solid #ef4444;
  border-radius: 5px;
  color: #fca5a5;
  font-size: 0.9375rem;
  cursor: pointer;

  &:hover { background: rgba(239, 68, 68, 0.25); }
}

/* ── Filename field ── */
.filename-row {
  display: flex;
  align-items: center;
  gap: 0.625rem;
}

.filename-label {
  font-size: 0.8125rem;
  color: #94a3b8;
  white-space: nowrap;
}

.filename-input {
  flex: 1;
  padding: 0.3rem 0.6rem;
  background: #0f1117;
  border: 1px solid #2d3248;
  border-radius: 4px;
  color: #e2e8f0;
  font-family: 'Fira Code', ui-monospace, monospace;
  font-size: 0.8125rem;

  &:focus { outline: none; border-color: #4f6ef7; }
}

/* ── Generate hint ── */
.gen-hint {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  flex-wrap: wrap;
  background: #0f1117;
  border: 1px solid #2d3248;
  border-radius: 4px;
  padding: 0.5rem 0.75rem;
}

.gen-label { font-size: 0.75rem; color: #64748b; white-space: nowrap; }

.gen-cmd {
  flex: 1;
  font-size: 0.75rem;
  color: #a5f3fc;
  word-break: break-all;
}

.btn-copy {
  padding: 0.2rem 0.6rem;
  background: none;
  border: 1px solid #2d3248;
  border-radius: 3px;
  color: #94a3b8;
  font-size: 0.75rem;
  cursor: pointer;
  white-space: nowrap;

  &:hover  { color: #e2e8f0; border-color: #4f6ef7; }
  &.copied { color: #6ee7b7; border-color: #34d399; }
}

.gen-note {
  margin: 0;
  font-size: 0.75rem;
  color: #64748b;
  code { color: #94a3b8; }
}

/* ── Textarea ── */
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

  &:focus { outline: none; border-color: #4f6ef7; }
}

/* ── Actions row ── */
.actions-row {
  display: flex;
  gap: 0.5rem;
  align-items: center;
}

.btn-file {
  padding: 0.375rem 0.75rem;
  background: none;
  border: 1px solid #2d3248;
  border-radius: 4px;
  color: #94a3b8;
  font-size: 0.8125rem;
  cursor: pointer;

  &:hover { color: #e2e8f0; border-color: #4f6ef7; }
}

.btn-generate {
  padding: 0.375rem 0.875rem;
  background: rgba(79, 110, 247, 0.12);
  border: 1px solid #4f6ef7;
  border-radius: 4px;
  color: #a5b4fc;
  font-size: 0.8125rem;
  cursor: pointer;

  &:hover:not(:disabled) { background: rgba(79, 110, 247, 0.22); color: #e0e7ff; }
  &:disabled { opacity: 0.5; cursor: not-allowed; }
}

/* ── Messages ── */
.msg-error { margin: 0; color: #fca5a5; font-size: 0.8125rem; }
</style>
