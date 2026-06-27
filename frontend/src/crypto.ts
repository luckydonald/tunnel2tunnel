// Offline Ed25519 key-pair generation using the Web Crypto API.
// Produces an OpenSSH private key file (PEM-wrapped) and an authorized_keys public key line.

function writeU32(n: number): Uint8Array {
  const b = new Uint8Array(4)
  new DataView(b.buffer).setUint32(0, n, false)
  return b
}

function writeStr(data: Uint8Array | string): Uint8Array {
  const bytes = typeof data === 'string' ? new TextEncoder().encode(data) : data
  return cat([writeU32(bytes.length), bytes])
}

function cat(arrays: Uint8Array[]): Uint8Array {
  const len = arrays.reduce((s, a) => s + a.length, 0)
  const out = new Uint8Array(len)
  let off = 0
  for (const a of arrays) { out.set(a, off); off += a.length }
  return out
}

function b64urlDecode(s: string): Uint8Array {
  const std = s.replace(/-/g, '+').replace(/_/g, '/').padEnd(Math.ceil(s.length / 4) * 4, '=')
  return Uint8Array.from(atob(std), c => c.charCodeAt(0))
}

function toBase64(bytes: Uint8Array): string {
  return btoa(String.fromCharCode(...bytes))
}

export async function generateEd25519KeyPair(comment: string): Promise<{
  privateKeyPem: string
  publicKeyLine: string
}> {
  // Ed25519 not yet in TypeScript's SubtleCrypto types — cast through unknown
  const kp = await crypto.subtle.generateKey(
    { name: 'Ed25519' } as unknown as EcKeyGenParams,
    true,
    ['sign', 'verify'] as unknown as KeyUsage[],
  )

  const privJwk = await crypto.subtle.exportKey('jwk', kp.privateKey)
  const pubBytes  = b64urlDecode(privJwk.x!)   // 32-byte public key
  const privBytes = b64urlDecode(privJwk.d!)   // 32-byte private scalar

  // OpenSSH stores: 32-byte private scalar || 32-byte public key  (64 bytes total)
  const privPub = cat([privBytes, pubBytes])

  // Public key blob  =  string("ssh-ed25519") + string(pubkey)
  const pubBlob = cat([writeStr('ssh-ed25519'), writeStr(pubBytes)])

  // Private section (unencrypted — ciphername "none", blocksize 8)
  const checkInt = new Uint8Array(4)
  crypto.getRandomValues(checkInt)

  const commentBytes = new TextEncoder().encode(comment)
  const privSectionBase = cat([
    checkInt,
    checkInt,                    // same uint32 twice for integrity check
    writeStr('ssh-ed25519'),
    writeStr(pubBytes),
    writeStr(privPub),           // 64 bytes
    writeStr(commentBytes),
  ])

  const padLen = (8 - (privSectionBase.length % 8)) % 8
  const padding = Uint8Array.from({ length: padLen }, (_, i) => i + 1)
  const privSection = cat([privSectionBase, padding])

  // Full OpenSSH private key binary
  const body = cat([
    new TextEncoder().encode('openssh-key-v1\0'),
    writeStr('none'),              // ciphername
    writeStr('none'),              // kdfname
    writeStr(new Uint8Array(0)),   // kdfoptions (empty)
    writeU32(1),                   // number of keys
    writeStr(pubBlob),             // public key
    writeStr(privSection),         // private section
  ])

  const b64 = toBase64(body).match(/.{1,70}/g)!.join('\n')
  const privateKeyPem = `-----BEGIN OPENSSH PRIVATE KEY-----\n${b64}\n-----END OPENSSH PRIVATE KEY-----\n`

  // authorized_keys public key line
  const publicKeyLine = `ssh-ed25519 ${toBase64(pubBlob)} ${comment}`

  return { privateKeyPem, publicKeyLine }
}

export function downloadText(filename: string, content: string): void {
  const blob = new Blob([content], { type: 'application/octet-stream' })
  const url = URL.createObjectURL(blob)
  const a = document.createElement('a')
  a.href = url
  a.download = filename
  a.click()
  URL.revokeObjectURL(url)
}
