//! Phase-0 feasibility proof: passwordless russh SSH server that splices a
//! "server" session and a "client" session together.
//!
//! Server connects:  ssh -R 8080:localhost:8080 server@localhost -p 2222 -N -o StrictHostKeyChecking=no
//! Client connects:  ssh -L 9090:server:8080   client@localhost -p 2222 -N -o StrictHostKeyChecking=no
//!
//! Any data sent into localhost:9090 flows to the server's port 8080 and back.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::Result;
use getrandom::rand_core::UnwrapErr;
use getrandom::SysRng;
use russh::keys::{Algorithm, PrivateKey};
use russh::server::{Auth, Config, Handle, Handler, Msg, Server, Session};
use russh::{Channel, ChannelId, ChannelMsg};
use tokio::sync::Mutex;

// ── Shared state ─────────────────────────────────────────────────────────────

#[derive(Default)]
struct AppState {
    /// port → handle of the SSH "server" client (the one doing -R)
    server_slots: HashMap<u32, Handle>,
    /// client-side channel id → (server handle, server channel id)
    bridges: HashMap<ChannelId, (Handle, ChannelId)>,
}

// ── Server factory ────────────────────────────────────────────────────────────

struct T2tServer {
    state: Arc<Mutex<AppState>>,
}

impl Server for T2tServer {
    type Handler = T2tHandler;

    fn new_client(&mut self, addr: Option<SocketAddr>) -> T2tHandler {
        println!("connect  from {addr:?}");
        T2tHandler {
            state: self.state.clone(),
            username: String::new(),
        }
    }
}

// ── Per-connection handler ────────────────────────────────────────────────────

struct T2tHandler {
    state: Arc<Mutex<AppState>>,
    username: String,
}

impl Handler for T2tHandler {
    type Error = anyhow::Error;

    // Accept every auth method without checking credentials.
    async fn auth_none(&mut self, user: &str) -> Result<Auth, Self::Error> {
        self.username = user.to_owned();
        println!("auth  user={user}  method=none  → accept");
        Ok(Auth::Accept)
    }

    async fn auth_password(&mut self, user: &str, _pw: &str) -> Result<Auth, Self::Error> {
        self.username = user.to_owned();
        println!("auth  user={user}  method=password  → accept");
        Ok(Auth::Accept)
    }

    async fn auth_publickey(
        &mut self,
        user: &str,
        _key: &russh::keys::PublicKey,
    ) -> Result<Auth, Self::Error> {
        self.username = user.to_owned();
        println!("auth  user={user}  method=publickey  → accept");
        Ok(Auth::Accept)
    }

    // "server" requests remote port forwarding: ssh -R port:... server@t2t
    async fn tcpip_forward(
        &mut self,
        address: &str,
        port: &mut u32,
        session: &mut Session,
    ) -> Result<bool, Self::Error> {
        println!("tcpip_forward  user={}  {}:{}", self.username, address, port);
        self.state
            .lock()
            .await
            .server_slots
            .insert(*port, session.handle());
        Ok(true)
    }

    async fn cancel_tcpip_forward(
        &mut self,
        _address: &str,
        port: u32,
        _session: &mut Session,
    ) -> Result<bool, Self::Error> {
        println!("cancel_tcpip_forward  port={port}");
        self.state.lock().await.server_slots.remove(&port);
        Ok(true)
    }

    // "client" opens a direct-tcpip channel: ssh -L local:server:port client@t2t
    async fn channel_open_direct_tcpip(
        &mut self,
        channel: Channel<Msg>,
        host_to_connect: &str,
        port_to_connect: u32,
        originator_address: &str,
        originator_port: u32,
        session: &mut Session,
    ) -> Result<bool, Self::Error> {
        println!(
            "direct_tcpip  user={}  dst={}:{}  src={}:{}",
            self.username, host_to_connect, port_to_connect, originator_address, originator_port
        );

        let server_handle = self
            .state
            .lock()
            .await
            .server_slots
            .get(&port_to_connect)
            .cloned();

        let Some(server_handle) = server_handle else {
            eprintln!("  no server registered on port {port_to_connect}");
            return Ok(false);
        };

        // Ask the server-side SSH client to open a forwarded-tcpip channel back
        // to the real service (e.g. the HTTP server on localhost:8080).
        let server_ch = server_handle
            .channel_open_forwarded_tcpip(
                host_to_connect,
                port_to_connect,
                originator_address,
                originator_port,
            )
            .await
            .map_err(|e| anyhow::anyhow!("channel_open_forwarded_tcpip failed: {e:?}"))?;

        let client_ch_id = channel.id();
        let server_ch_id = server_ch.id();
        println!("  bridge  client#{client_ch_id} <-> server#{server_ch_id}");

        self.state
            .lock()
            .await
            .bridges
            .insert(client_ch_id, (server_handle.clone(), server_ch_id));

        // Spawn task: copy data from server channel → client session.
        let client_handle = session.handle();
        tokio::spawn(forward(server_ch, client_handle, client_ch_id));

        Ok(true)
    }

    // Data from client → forward to server channel.
    async fn data(
        &mut self,
        channel: ChannelId,
        data: &[u8],
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        let state = self.state.lock().await;
        if let Some((handle, ch)) = state.bridges.get(&channel) {
            let handle = handle.clone();
            let ch = *ch;
            let payload = data.to_vec();
            drop(state);
            let _ = handle.data(ch, payload).await;
        }
        Ok(())
    }

    async fn channel_eof(
        &mut self,
        channel: ChannelId,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        let state = self.state.lock().await;
        if let Some((handle, ch)) = state.bridges.get(&channel) {
            let handle = handle.clone();
            let ch = *ch;
            drop(state);
            let _ = handle.eof(ch).await;
        }
        Ok(())
    }

    async fn channel_close(
        &mut self,
        channel: ChannelId,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        let mut state = self.state.lock().await;
        if let Some((handle, ch)) = state.bridges.remove(&channel) {
            drop(state);
            let _ = handle.close(ch).await;
        }
        Ok(())
    }
}

// ── Channel forwarding task (server → client) ─────────────────────────────────

async fn forward(mut src: Channel<Msg>, dst: Handle, dst_ch: ChannelId) {
    loop {
        match src.wait().await {
            Some(ChannelMsg::Data { data }) => {
                if dst.data(dst_ch, data.as_ref().to_vec()).await.is_err() {
                    break;
                }
            }
            Some(ChannelMsg::Eof) | None => {
                let _ = dst.eof(dst_ch).await;
                break;
            }
            Some(ChannelMsg::Close) => {
                let _ = dst.close(dst_ch).await;
                break;
            }
            _ => {}
        }
    }
    println!("forward task done  dst_ch={dst_ch}");
}

// ── Entry point ───────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    let key = PrivateKey::random(&mut UnwrapErr(SysRng), Algorithm::Ed25519)
        .expect("keygen failed");

    let config = Arc::new(Config {
        keys: vec![key],
        ..Config::default()
    });

    let state = Arc::new(Mutex::new(AppState::default()));
    let mut server = T2tServer { state };

    println!("t2t-feasibility  listening on 0.0.0.0:2222");
    println!();
    println!("  Server side:  ssh -R 8080:localhost:8080 server@localhost -p 2222 -N -o StrictHostKeyChecking=no");
    println!("  Client side:  ssh -L 9090:server:8080   client@localhost -p 2222 -N -o StrictHostKeyChecking=no");
    println!();
    println!("  Then:  curl http://localhost:9090  (routes through the bridge to server's :8080)");

    server.run_on_address(config, ("0.0.0.0", 2222)).await?;
    Ok(())
}
