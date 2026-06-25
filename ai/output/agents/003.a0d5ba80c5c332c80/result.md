Perfect! I now have comprehensive information. Let me compile the final report:

## russh Crate API Reference Report

Based on my research of the official russh documentation on docs.rs, here is the complete API specification you requested:

### **Crate Version**
- **Latest stable version: 0.61.2** (available on crates.io)

---

### **1. Handler Trait Method Signatures**

All methods return `impl Future<Output = Result<T, Self::Error>> + Send` and use **native async trait syntax** (NOT `#[async_trait]` attribute required).

**Authentication Methods:**
```rust
fn auth_none(&mut self, user: &str) 
  -> impl Future<Output = Result<Auth, Self::Error>> + Send

fn auth_publickey(&mut self, user: &str, public_key: &PublicKey) 
  -> impl Future<Output = Result<Auth, Self::Error>> + Send

fn auth_password(&mut self, user: &str, password: &str) 
  -> impl Future<Output = Result<Auth, Self::Error>> + Send
```

**Port Forwarding Methods:**
```rust
fn tcpip_forward(&mut self, address: &str, port: &mut u32, session: &mut Session) 
  -> impl Future<Output = Result<bool, Self::Error>> + Send

fn cancel_tcpip_forward(&mut self, address: &str, port: u32, session: &mut Session) 
  -> impl Future<Output = Result<bool, Self::Error>> + Send

fn channel_open_direct_tcpip(
    &mut self, 
    channel: Channel<Msg>, 
    host_to_connect: &str, 
    port_to_connect: u32, 
    originator_address: &str, 
    originator_port: u32, 
    session: &mut Session
) -> impl Future<Output = Result<bool, Self::Error>> + Send
```

**Channel Data Methods:**
```rust
fn data(&mut self, channel: ChannelId, data: &[u8], session: &mut Session) 
  -> impl Future<Output = Result<(), Self::Error>> + Send

fn channel_eof(&mut self, channel: ChannelId, session: &mut Session) 
  -> impl Future<Output = Result<(), Self::Error>> + Send

fn channel_close(&mut self, channel: ChannelId, session: &mut Session) 
  -> impl Future<Output = Result<(), Self::Error>> + Send
```

**Handler Trait Notes:**
- Has **no required methods** — all 34 methods have default implementations
- Has **one required associated type:** `Error: From<Error> + Send`
- Use `async fn` directly in impl blocks (native async trait, not macro-based)

---

### **2. Handle Struct Methods**

All methods are `async fn` (not futures):

```rust
pub async fn data(&self, id: ChannelId, data: impl Into<Bytes>) 
  -> Result<(), Bytes>

pub async fn eof(&self, id: ChannelId) 
  -> Result<(), ()>

pub async fn close(&self, id: ChannelId) 
  -> Result<(), ()>

pub async fn channel_open_forwarded_tcpip<A: Into<String>, B: Into<String>>(
    &self,
    connected_address: A,
    connected_port: u32,
    originator_address: B,
    originator_port: u32
) -> Result<Channel<Msg>, Error>
```

---

### **3. ChannelMsg Enum Variants**

Key variants with their data types:

```rust
// Core variants
ChannelMsg::Data(Bytes)           // Contains transmitted bytes
ChannelMsg::Eof                   // End-of-file signal (no data)
ChannelMsg::Close                 // Channel closure (no data)
ChannelMsg::ExtendedData(u32, Bytes)  // Extended data with type identifier

// Channel control
ChannelMsg::Open { id, max_packet_size, window_size }
ChannelMsg::Success
ChannelMsg::Failure
ChannelMsg::OpenFailure(ChannelOpenFailure)

// Terminal/Shell (client-only)
ChannelMsg::RequestPty { ... }
ChannelMsg::RequestShell
ChannelMsg::Exec(Bytes)
ChannelMsg::RequestSubsystem(Bytes)

// Signals & Flow Control
ChannelMsg::Signal(Sig)
ChannelMsg::WindowAdjusted
ChannelMsg::WindowChange { ... }

// Process termination
ChannelMsg::ExitStatus(u32)
ChannelMsg::ExitSignal { ... }
```

The enum is marked `#[non_exhaustive]`, allowing future additions without breaking existing code.

---

### **4. Host Key Management**

**Loading existing keys:**
```rust
use russh::keys::{load_secret_key, PrivateKey};

pub fn load_secret_key<P: AsRef<Path>>(
    secret_: P,
    password: Option<&str>,
) -> Result<PrivateKey, Error>
```

**Generating new keys:**
```rust
use russh::keys::PrivateKey;

// From PrivateKey methods:
PrivateKey::random(algorithm: Algorithm) -> PrivateKey
PrivateKey::from_openssh(data: &str, password: Option<&str>) -> Result<Self, Error>
PrivateKey::read_openssh_file<P: AsRef<Path>>(path: P) -> Result<Self, Error>
```

**Server configuration:**
```rust
use russh::server::Config;
use std::sync::Arc;

let mut config = Config::default();
config.keys = vec![private_key];  // Vec<PrivateKey>

let config = Arc::new(config);
```

---

### **5. Server Startup**

**Using the Server trait:**
```rust
impl Server for MyServer {
    type Handler = MyHandler;
    
    fn new_client(&mut self, _: Option<SocketAddr>) -> Self::Handler {
        MyHandler::new()
    }
}

// Run on address:
server.run_on_address(config.clone(), ("0.0.0.0", 2222)).await?

// Run on existing listener:
server.run_on_socket(config.clone(), listener).await?
```

**For single connections:**
```rust
pub fn run_stream(
    read: impl AsyncRead,
    write: impl AsyncWrite,
    config: Arc<Config>,
    handler: Handler,
) -> impl Future<Output = Result<()>>
```

---

### **Key Type Exports**
From `russh::keys` module:
- `PrivateKey` — SSH private key structure
- `PublicKey` — SSH public key structure
- `Algorithm` enum — Signature algorithms
- `EcdsaCurve` enum — ECDSA elliptic curves
- `Certificate` — OpenSSH certificates
- Error, Encryption, HashAlg enums

From `russh::server` module:
- `Handler` trait
- `Server` trait
- `Config` struct
- `Handle` struct
- `Session` struct
- `Auth` enum
- `run_stream` function

---

### **Summary**
- **Version:** 0.61.2
- **Async trait:** Native `impl Future` (no `#[async_trait]` macro needed)
- **Host keys:** Load with `load_secret_key()`, generate with `PrivateKey::random()`, configure via `Config::keys`
- **Server:** Implement `Server` trait with `new_client()`, call `run_on_address()` or `run_on_socket()`
- **Handler:** Implement with native `async fn`, no required methods (all have defaults)