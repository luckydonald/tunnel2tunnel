Look up the russh crate (Rust SSH library) on crates.io or docs.rs to find the current stable version and the exact API signatures for:
1. `russh::server::Handler` trait — specifically `auth_none`, `auth_publickey`, `auth_password`, `tcpip_forward`, `cancel_tcpip_forward`, `channel_open_direct_tcpip`, `data`, `channel_eof`, `channel_close` method signatures
2. `russh::server::Handle` methods — specifically `data`, `eof`, `close`, `channel_open_forwarded_tcpip`
3. `russh::ChannelMsg` variants — especially `Data`, `Eof`, `Close`
4. How to generate a host key (russh_keys or russh::keys)
5. How to call `russh::server::run` or equivalent to start the server
6. Whether `#[async_trait]` is needed on the Handler impl (or if native async traits are used)
7. The latest version of russh available on crates.io

Report exact function signatures and the crate version. Check docs.rs if possible.