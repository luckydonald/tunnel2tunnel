---
name: ssh_key_persistence
description: ssh-key 0.7 PrivateKey encryption API; host key persistence pattern for russh servers
metadata: 
  node_type: memory
  type: project
  originSessionId: 4e03ed8c-f268-407f-a241-415199236b9b
---

## ssh-key 0.7 encryption API (via russh::keys)

All methods are on `russh::keys::PrivateKey` (re-exports `ssh_key::PrivateKey`):

```rust
// Check if a loaded key is encrypted
key.is_encrypted() -> bool

// Decrypt an encrypted key (returns a new decrypted PrivateKey)
key.decrypt(password: impl AsRef<[u8]>) -> Result<PrivateKey>

// Encrypt a plaintext key (AES-256-CTR by default)
key.encrypt(rng: &mut impl TryCryptoRng, password: impl AsRef<[u8]>) -> Result<PrivateKey>

// Read/write OpenSSH PEM format
PrivateKey::read_openssh_file(path) -> Result<PrivateKey>
key.write_openssh_file(path, LineEnding::LF) -> Result<()>  // sets 0o600 permissions
```

`LineEnding` lives at `russh::keys::ssh_key::LineEnding`.

**Why:** The `encryption` feature is enabled in russh 0.61's ssh-key dependency, so all of the above are available without adding a direct ssh-key dep.

## Host key persistence pattern

Generate once, reload on restart — avoids "REMOTE HOST IDENTIFICATION HAS CHANGED!" errors:

```rust
fn load_or_generate_host_key(path: &str, password: Option<&str>) -> Result<PrivateKey> {
    if Path::new(path).exists() {
        let key = PrivateKey::read_openssh_file(path)?;
        Ok(if key.is_encrypted() { key.decrypt(password.unwrap())? } else { key })
    } else {
        let key = PrivateKey::random(&mut UnwrapErr(SysRng), Algorithm::Ed25519)?;
        let to_save = match password { Some(pw) => key.encrypt(&mut UnwrapErr(SysRng), pw)?, None => key.clone() };
        std::fs::create_dir_all(Path::new(path).parent().unwrap())?;
        to_save.write_openssh_file(path, LineEnding::LF)?;
        Ok(key)
    }
}
```

Mount the key's parent directory as a Docker volume (`t2t_data:/data`).
