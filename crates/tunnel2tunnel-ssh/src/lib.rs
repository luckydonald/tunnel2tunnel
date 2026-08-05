use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use getrandom::rand_core::UnwrapErr;
use getrandom::SysRng;
use russh::keys::ssh_key::LineEnding;
use russh::keys::{Algorithm, PrivateKey};
use russh::server::{Auth, Config, Handle, Handler, Msg, Server, Session};
use russh::{Channel, ChannelId, ChannelMsg, Preferred, Pty};
use russh::{MethodKind, MethodSet};
use serde::Serialize;
use sqlx::PgPool;
use tokio::net::TcpListener;
use tokio::sync::{broadcast, Mutex};
use uuid::Uuid;

use tunnel2tunnel_core::{
    ip_whitelist,
    models::{
        connection_log::ConnectionLog, entity::Entity, entity_access::EntityAccess,
        port_config::PortConfig, port_subscription::PortSubscription, ssh_key::SshKey,
    },
    port_names::{guess_service_name, slugify_host},
};

mod tarpit;
use tarpit::{SharedThresholds, TarpitMethod, TarpitState};

pub struct SshConfig {
    pub ssh_port: u16,
    pub fail2ban_log_path: Option<String>,
    /// Path on disk where the SSH host key is stored. Generated on first run.
    pub host_key_path: String,
    /// Optional passphrase used to encrypt the stored host key (SSH_T2T_KEY_PASSWORD).
    pub host_key_password: Option<String>,
}

// ── Routing state shared across all SSH sessions ─────────────────────────────

pub type ServerSlots = Arc<Mutex<HashMap<(Uuid, u32), (Handle, String)>>>;

/// Info about one currently-bridged client-side tunnel (a live
/// `channel_open_direct_tcpip` bridge), aggregated across all SSH
/// connections — keyed by a fresh bridge id (`Uuid::now_v7()`), not by
/// `ChannelId`, which is only unique within a single connection. Consumed by
/// `tunnel2tunnel-web`'s `routes/live_connections.rs` for the live-connections
/// dashboard/admin views.
#[derive(Clone, Debug)]
pub struct ActiveTunnelInfo {
    pub client_entity_id: Uuid,
    pub client_user_id: Uuid,
    pub target_entity_id: Uuid,
    /// Which `port_configs` row (owned by the target entity) this bridge is
    /// using.
    pub port_config_id: Uuid,
    pub proxy_port: u32,
    pub peer_ip: String,
    pub since: time::OffsetDateTime,
}

pub type ActiveTunnels = Arc<Mutex<HashMap<Uuid, ActiveTunnelInfo>>>;

/// Fired (empty payload — just a "something changed" ping) whenever
/// `ServerSlots`/`ActiveTunnels`/online-status state mutates, so
/// `tunnel2tunnel-web`'s live-connections WebSocket routes can push a fresh
/// snapshot instead of the client having to poll. No receivers subscribed is
/// fine — `send` returning an error just means nobody's listening right now.
pub type LiveUpdateTx = broadcast::Sender<()>;

/// Constructs a fresh `LiveUpdateTx` — see `new_server_slots` for the sharing
/// pattern (one instance handed to both the SSH server and the web `AppState`).
pub fn new_live_update_tx() -> LiveUpdateTx {
    broadcast::channel(16).0
}

/// A discrete "this specific thing changed" notification, emitted right at
/// the SSH-side mutation site where old/new state is already known — see
/// each call site of `LiveEventTx::send` below for exactly which transition
/// it represents. Consumed by `tunnel2tunnel-web`'s unified
/// `/api/live-connections/ws` route to drive both resync timing and
/// frontend toast notifications (the `reason` field on that route's
/// envelope). Deliberately minimal payloads (ids + port only, no names) —
/// consumers already have entity/service names from the snapshot on the
/// same connection and enrich from a local lookup instead of this hot path
/// doing extra DB work.
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LiveEvent {
    EntityOnline {
        entity_id: Uuid,
    },
    EntityOffline {
        entity_id: Uuid,
    },
    PortForwardingStarted {
        entity_id: Uuid,
        proxy_port: u32,
    },
    PortForwardingStopped {
        entity_id: Uuid,
        proxy_port: u32,
    },
    BridgeStarted {
        client_entity_id: Uuid,
        target_entity_id: Uuid,
        proxy_port: u32,
    },
    BridgeStopped {
        client_entity_id: Uuid,
        target_entity_id: Uuid,
        proxy_port: u32,
    },
}

impl LiveEvent {
    /// Every entity id this event concerns — used by the web layer to decide
    /// whether a given connection (scoped to one user's own entities) should
    /// see this event at all.
    pub fn entity_ids(&self) -> Vec<Uuid> {
        match self {
            LiveEvent::EntityOnline { entity_id } | LiveEvent::EntityOffline { entity_id } => {
                vec![*entity_id]
            }
            LiveEvent::PortForwardingStarted { entity_id, .. }
            | LiveEvent::PortForwardingStopped { entity_id, .. } => vec![*entity_id],
            LiveEvent::BridgeStarted {
                client_entity_id,
                target_entity_id,
                ..
            }
            | LiveEvent::BridgeStopped {
                client_entity_id,
                target_entity_id,
                ..
            } => vec![*client_entity_id, *target_entity_id],
        }
    }
}

pub type LiveEventTx = broadcast::Sender<LiveEvent>;

/// Constructs a fresh `LiveEventTx` — see `new_live_update_tx` for the
/// sharing pattern.
pub fn new_live_event_tx() -> LiveEventTx {
    broadcast::channel(64).0
}

/// Client-side bridge channel -> (paired server handle, paired server-side
/// channel id). `Arc<Mutex<_>>` rather than a plain field because bridge
/// setup for a single `channel_open_direct_tcpip` request now happens in a
/// `tokio::spawn`ed task (see that handler) rather than inline in the
/// `Handler` callback, so it can no longer rely on holding `&mut self`.
type Bridges = Arc<Mutex<HashMap<ChannelId, (Handle, ChannelId)>>>;

/// Client-side bridge channel -> its `ActiveTunnels` key, so the entry can be
/// removed again on `channel_close`/`Drop`. Same `Arc<Mutex<_>>` reasoning as
/// `Bridges` above.
type BridgeIds = Arc<Mutex<HashMap<ChannelId, Uuid>>>;

/// Constructs a fresh, empty `ServerSlots` map. Exposed so `crates/t2t`'s
/// `main.rs` can build one instance and share it between the SSH server and
/// the web `AppState` (both need to read/write the same map).
pub fn new_server_slots() -> ServerSlots {
    Arc::new(Mutex::new(HashMap::new()))
}

/// Constructs a fresh, empty `ActiveTunnels` map — see `new_server_slots`.
pub fn new_active_tunnels() -> ActiveTunnels {
    Arc::new(Mutex::new(HashMap::new()))
}

// ── Session registry for messaging (welcome, ping, broadcast, chat) ──────────

struct SessionEntry {
    handle: Handle,
    /// Channel used for push messages (welcome, ping, notifications).
    session_channel_id: Option<ChannelId>,
}

type SessionRegistry = Arc<Mutex<HashMap<Uuid, SessionEntry>>>;

/// Send `msg` to every registered session channel, optionally skipping one connection.
async fn broadcast(registry: &SessionRegistry, msg: &str, exclude: Option<Uuid>) {
    let targets: Vec<(Handle, ChannelId)> = {
        let reg = registry.lock().await;
        reg.iter()
            .filter(|(conn_id, _)| exclude.map_or(true, |ex| **conn_id != ex))
            .filter_map(|(_, entry)| {
                entry
                    .session_channel_id
                    .map(|ch| (entry.handle.clone(), ch))
            })
            .collect()
    };
    for (handle, ch) in targets {
        let _ = handle.data(ch, msg.as_bytes().to_vec()).await;
    }
}

// ── Entry point ───────────────────────────────────────────────────────────────

/// Return the SHA-256 fingerprint of the configured SSH host key,
/// generating and persisting it first if it doesn't yet exist.
pub fn host_key_fingerprint(path: &str, password: Option<&str>) -> Result<String> {
    let key = load_or_generate_host_key(path, password)?;
    Ok(format!(
        "{}",
        key.public_key()
            .fingerprint(russh::keys::ssh_key::HashAlg::Sha256)
    ))
}

pub async fn start(
    config: SshConfig,
    pool: PgPool,
    server_slots: ServerSlots,
    active_tunnels: ActiveTunnels,
    live_update_tx: LiveUpdateTx,
    live_event_tx: LiveEventTx,
) -> Result<()> {
    let key =
        load_or_generate_host_key(&config.host_key_path, config.host_key_password.as_deref())?;

    let russh_config = Arc::new(Config {
        keys: vec![key],
        // russh 0.61.2's delayed `zlib@openssh.com` compression (negotiated whenever a
        // client offers it, e.g. plain OpenSSH with `Compression yes`) corrupts packet
        // framing shortly after auth completes — observed in production as sessions
        // dying a few seconds in with `SshEncoding: length invalid` (see ai/errors/7.t2t.md).
        // Restricting to "none" keeps negotiation from ever picking zlib.
        preferred: Preferred {
            compression: std::borrow::Cow::Borrowed(&[russh::compression::NONE]),
            ..Preferred::default()
        },
        ..Config::default()
    });

    let session_registry: SessionRegistry = Arc::new(Mutex::new(HashMap::new()));
    let fail2ban = config.fail2ban_log_path.map(Arc::new);
    let tarpit_state: TarpitState = tarpit::new_state();
    let thresholds: SharedThresholds = Arc::new(Mutex::new(tarpit::ThresholdConfig::default()));

    tarpit::spawn_settings_refresher(pool.clone(), tarpit_state.clone(), thresholds.clone()).await;

    let mut server = T2tServer {
        pool: pool.clone(),
        server_slots,
        active_tunnels,
        live_update_tx,
        live_event_tx,
        session_registry,
        fail2ban,
        tarpit: tarpit_state,
        thresholds,
    };

    // A hand-rolled accept loop (rather than `Server::run_on_address`) is
    // required here: `russh::server::run_stream` unconditionally writes the
    // real "SSH-2.0-..." identification string as its first action, before
    // any `Handler` callback ever runs — there is no hook to intercept it.
    // The endlessh-style banner-drip tarpit (which must never send that real
    // line) therefore has to own the raw `TcpStream` itself, decided here at
    // accept time, before the socket is ever handed to russh.
    let listener = TcpListener::bind(("0.0.0.0", config.ssh_port)).await?;
    tracing::info!(port = config.ssh_port, "SSH server listening");

    loop {
        let (socket, addr) = match listener.accept().await {
            Ok(pair) => pair,
            Err(e) => {
                tracing::warn!(err = %e, "SSH: accept() failed");
                continue;
            }
        };
        let peer_ip = addr.ip().to_string();

        let pre_auth_outcome = tarpit::decide_pre_auth_tarpit(
            &server.tarpit,
            &server.thresholds,
            &server.pool,
            &peer_ip,
        )
        .await;

        if let tarpit::TarpitOutcome::Ban { source } = pre_auth_outcome {
            tracing::info!(%peer_ip, "SSH: hard ban engaged for new connection — closing instantly");
            tokio::spawn(tarpit::log_hard_ban(server.pool.clone(), peer_ip, source));
            continue;
        }

        if let tarpit::TarpitOutcome::Trap {
            method: TarpitMethod::BannerDrip,
            source,
        } = &pre_auth_outcome
        {
            let pool = server.pool.clone();
            let fail2ban = server.fail2ban.clone();
            let source = source.clone();
            tracing::info!(%peer_ip, "SSH: banner-drip tarpit engaged for new connection");
            tokio::spawn(tarpit::banner_drip::run(
                socket,
                peer_ip,
                pool,
                fail2ban,
                "ip banned",
                source,
            ));
            continue;
        }

        let mut handler = server.new_client(Some(addr));
        // Only cache a *positive* accept-time decision (this peer_ip is
        // already banned from prior connections) — reused for every auth
        // attempt on this connection, same as before. If it's `None` (not
        // yet banned), leave `tarpit_outcome` unset so the connection's own
        // first auth failure still gets a fresh, up-to-the-moment decision
        // in `resolve_tarpit_outcome` (see its doc comment): with the tally
        // now recorded there *before* deciding, a `fail_count = 1` rule can
        // trip on this very first attempt instead of only the next connection.
        if !matches!(pre_auth_outcome, tarpit::TarpitOutcome::None) {
            handler.tarpit_outcome = Some(pre_auth_outcome);
        }
        let cfg = russh_config.clone();
        tokio::spawn(async move {
            match russh::server::run_stream(cfg, socket, handler).await {
                Ok(session) => {
                    if let Err(e) = session.await {
                        tracing::warn!(err = %e, "SSH: session ended with error");
                    }
                }
                Err(e) => tracing::debug!(err = %e, "SSH: connection setup failed"),
            }
        });
    }
}

// ── Server factory ─────────────────────────────────────────────────────────

struct T2tServer {
    pool: PgPool,
    server_slots: ServerSlots,
    active_tunnels: ActiveTunnels,
    live_update_tx: LiveUpdateTx,
    live_event_tx: LiveEventTx,
    session_registry: SessionRegistry,
    fail2ban: Option<Arc<String>>,
    tarpit: TarpitState,
    thresholds: SharedThresholds,
}

impl Server for T2tServer {
    type Handler = T2tHandler;

    fn new_client(&mut self, addr: Option<SocketAddr>) -> T2tHandler {
        let peer_ip = addr
            .map(|a| a.ip().to_string())
            .unwrap_or_else(|| "unknown".into());
        let conn_id = Uuid::now_v7();
        tracing::info!(%peer_ip, %conn_id, "SSH: new connection");
        T2tHandler {
            pool: self.pool.clone(),
            server_slots: self.server_slots.clone(),
            active_tunnels: self.active_tunnels.clone(),
            live_update_tx: self.live_update_tx.clone(),
            live_event_tx: self.live_event_tx.clone(),
            session_registry: self.session_registry.clone(),
            fail2ban: self.fail2ban.clone(),
            tarpit: self.tarpit.clone(),
            thresholds: self.thresholds.clone(),
            conn_id,
            peer_ip,
            entity: None,
            bridges: Arc::new(Mutex::new(HashMap::new())),
            bridge_ids: Arc::new(Mutex::new(HashMap::new())),
            connection_log_ids: Vec::new(),
            tarpit_outcome: None,
            fake_shell: false,
            chat_input: Vec::new(),
        }
    }
}

// ── Authenticated entity info ────────────────────────────────────────────────

#[derive(Clone)]
struct AuthedEntity {
    entity: Entity,
    user_id: Uuid,
}

// ── Per-connection handler ────────────────────────────────────────────────────

struct T2tHandler {
    pool: PgPool,
    server_slots: ServerSlots,
    active_tunnels: ActiveTunnels,
    live_update_tx: LiveUpdateTx,
    live_event_tx: LiveEventTx,
    session_registry: SessionRegistry,
    fail2ban: Option<Arc<String>>,
    tarpit: TarpitState,
    thresholds: SharedThresholds,
    conn_id: Uuid,
    peer_ip: String,
    entity: Option<AuthedEntity>,
    bridges: Bridges,
    /// Maps this connection's client-side bridge channels to the
    /// `active_tunnels` key registered for them, so the entry can be removed
    /// again on `channel_close`/`Drop`.
    bridge_ids: BridgeIds,
    /// ids of every `connection_logs` row created for this TCP connection
    /// (successful login and/or any number of failed/trap attempts before
    /// it) — all of them get `ended_at` stamped together in `Drop`.
    connection_log_ids: Vec<Uuid>,
    /// Decided once per connection (either at accept time for a peer_ip-level
    /// ban, or lazily on first real auth attempt for a user-level ban).
    /// `Trap{method: BannerDrip, ..}` never appears here — by the time any
    /// `Handler` callback runs, the real SSH-2.0 line has already gone out
    /// (a pre-auth `BannerDrip` decision is acted on directly in the accept
    /// loop and never reaches a handler at all).
    tarpit_outcome: Option<tarpit::TarpitOutcome>,
    /// Set when this connection was waved through `Auth::Accept` as a
    /// fake-shell trap rather than a real login — `entity` stays `None`.
    fake_shell: bool,
    /// Line-in-progress for the chat text channel. No real pty is allocated
    /// (see `pty_request`'s doc comment), so the client's terminal sends raw,
    /// unbuffered bytes rather than one full line per `data()` call — this
    /// buffer accumulates keystrokes until Enter, and every byte is echoed
    /// back to the sender so they can see what they've typed so far. Kept as
    /// raw bytes (not `String`) since a multi-byte UTF-8 character can arrive
    /// split across separate `data()` calls.
    chat_input: Vec<u8>,
}

impl T2tHandler {
    /// Lazily resolves (and caches) which tarpit outcome applies to this
    /// connection's auth attempts, once we're already inside a `Handler`
    /// callback (banner-drip is not selectable here — see `tarpit_outcome`'s
    /// doc comment).
    async fn resolve_tarpit_outcome(&mut self, user_id: Option<Uuid>) -> tarpit::TarpitOutcome {
        if self.tarpit_outcome.is_none() {
            self.tarpit_outcome = Some(
                tarpit::decide_in_auth_tarpit(
                    &self.tarpit,
                    &self.thresholds,
                    &self.peer_ip,
                    user_id,
                )
                .await,
            );
        }
        self.tarpit_outcome
            .clone()
            .unwrap_or(tarpit::TarpitOutcome::None)
    }

    /// Records this failed attempt's tally. Always called — regardless of
    /// whether `tarpit_outcome` is already cached for this connection — so
    /// tallies (and round-robin trigger-count escalation across repeated
    /// bans) keep advancing even once a connection is already known-banned.
    /// Must run *before* `resolve_tarpit_outcome` at every call site: for a
    /// connection whose ban status isn't cached yet (see accept loop in
    /// `start()`), that's what lets a `fail_count = 1` rule trip on this very
    /// attempt instead of only the next connection.
    async fn record_this_failure(&self, user_id: Option<Uuid>) {
        let thresholds = self.thresholds.lock().await.clone();
        tarpit::record_auth_failure(&self.tarpit, &self.peer_ip, user_id, &thresholds).await;
    }

    #[allow(clippy::too_many_arguments)]
    async fn log_auth_failure(
        &mut self,
        user_id: Option<Uuid>,
        fingerprint: Option<&str>,
        attempted_password: Option<&str>,
        attempted_username: &str,
        reason: &str,
        outcome: &tarpit::TarpitOutcome,
        // The tally is now recorded inside `resolve_tarpit_outcome` (called by
        // every caller before `log_auth_failure`), so it can factor into the
        // same attempt's trap/ban decision. Kept for call-site clarity only.
        _counts_toward_ban: bool,
    ) {
        let (tarpit_method, tarpit_action, source) = match outcome {
            tarpit::TarpitOutcome::Trap { method, source } => {
                (Some(method.as_str()), Some("trap"), source.clone())
            }
            tarpit::TarpitOutcome::Ban { source } => (None, Some("ban"), Some(source.clone())),
            tarpit::TarpitOutcome::None => (None, None, None),
        };
        let (threshold_id, ban_rule_id) = match source {
            Some(tarpit::BanSource::Threshold(id)) => (Some(id), None),
            Some(tarpit::BanSource::AdminRule(id)) => (None, Some(id)),
            None => (None, None),
        };

        let now = time::OffsetDateTime::now_utc();
        match ConnectionLog::create(
            &self.pool,
            None,
            user_id,
            Some(&self.peer_ip),
            fingerprint,
            attempted_password,
            Some(attempted_username),
            Some(reason),
            None,
            tarpit_method,
            tarpit_action,
            threshold_id,
            ban_rule_id,
            now,
        )
        .await
        {
            Ok(log) => self.connection_log_ids.push(log.id),
            Err(e) => tracing::warn!(err = %e, "failed to write auth failure log"),
        }

        if let Some(path) = &self.fail2ban {
            let fp = fingerprint.unwrap_or("unknown");
            let line = format!(
                "{} t2t sshd[0]: Failed publickey for invalid user {} from {} port 0 ssh2\n",
                chrono_like_timestamp(),
                fp,
                self.peer_ip,
            );
            if let Err(e) = append_to_file(path, &line).await {
                tracing::warn!(err = %e, "fail2ban write failed");
            }
        }
    }

    async fn log_auth_success(
        &mut self,
        entity: &Entity,
        fingerprint: &str,
        attempted_username: &str,
    ) {
        let now = time::OffsetDateTime::now_utc();
        match ConnectionLog::create(
            &self.pool,
            Some(entity.id),
            Some(entity.user_id),
            Some(&self.peer_ip),
            Some(fingerprint),
            None,
            Some(attempted_username),
            None,
            Some("correct login"),
            None,
            None,
            None,
            None,
            now,
        )
        .await
        {
            Ok(log) => self.connection_log_ids.push(log.id),
            Err(e) => tracing::warn!(err = %e, "failed to write auth success log"),
        }

        tarpit::record_auth_success(&self.tarpit, &self.peer_ip, Some(entity.user_id)).await;
        let _ = self.live_update_tx.send(());
        let _ = self.live_event_tx.send(LiveEvent::EntityOnline {
            entity_id: entity.id,
        });

        if let Some(path) = &self.fail2ban {
            let line = format!(
                "{} t2t sshd[0]: Accepted publickey for {} from {} port 0 ssh2\n",
                chrono_like_timestamp(),
                entity.id,
                self.peer_ip,
            );
            if let Err(e) = append_to_file(path, &line).await {
                tracing::warn!(err = %e, "fail2ban write failed");
            }
        }
    }
}

impl Handler for T2tHandler {
    type Error = anyhow::Error;

    async fn auth_none(&mut self, user: &str) -> Result<Auth, Self::Error> {
        tracing::info!(
            peer_ip = %self.peer_ip,
            %user,
            available = "publickey",
            "SSH: auth attempt (none) — rejected"
        );
        Ok(Auth::Reject {
            proceed_with_methods: Some(publickey_only()),
            partial_success: false,
        })
    }

    async fn auth_password(&mut self, user: &str, password: &str) -> Result<Auth, Self::Error> {
        tracing::info!(
            peer_ip = %self.peer_ip,
            %user,
            password_len = password.len(),
            available = "publickey",
            "SSH: auth attempt (password) — rejected (password auth not supported)"
        );
        self.record_this_failure(None).await;
        let outcome = self.resolve_tarpit_outcome(None).await;
        self.log_auth_failure(
            None,
            None,
            Some(password),
            user,
            "password auth not supported",
            &outcome,
            true,
        )
        .await;

        if let tarpit::TarpitOutcome::Trap { method, .. } = outcome {
            if method == TarpitMethod::FakeShell {
                self.fake_shell = true;
                return Ok(Auth::Accept);
            }
            if method == TarpitMethod::SlowAuth {
                tarpit::slow_auth::delay().await;
            }
        }
        Ok(Auth::Reject {
            proceed_with_methods: Some(publickey_only()),
            partial_success: false,
        })
    }

    async fn auth_keyboard_interactive(
        &mut self,
        user: &str,
        submethods: &str,
        _response: Option<russh::server::Response<'_>>,
    ) -> Result<Auth, Self::Error> {
        tracing::info!(
            peer_ip = %self.peer_ip,
            %user,
            %submethods,
            available = "publickey",
            "SSH: auth attempt (keyboard-interactive) — rejected"
        );
        self.record_this_failure(None).await;
        let outcome = self.resolve_tarpit_outcome(None).await;
        self.log_auth_failure(
            None,
            None,
            None,
            user,
            "unsupported auth method",
            &outcome,
            true,
        )
        .await;

        if let tarpit::TarpitOutcome::Trap { method, .. } = outcome {
            if method == TarpitMethod::FakeShell {
                self.fake_shell = true;
                return Ok(Auth::Accept);
            }
            if method == TarpitMethod::SlowAuth {
                tarpit::slow_auth::delay().await;
            }
        }
        Ok(Auth::Reject {
            proceed_with_methods: Some(publickey_only()),
            partial_success: false,
        })
    }

    async fn auth_publickey_offered(
        &mut self,
        user: &str,
        key: &russh::keys::PublicKey,
    ) -> Result<Auth, Self::Error> {
        let fp = format!("{}", key.fingerprint(russh::keys::ssh_key::HashAlg::Sha256));
        tracing::info!(
            peer_ip = %self.peer_ip,
            %user,
            %fp,
            key_algo = key.algorithm().as_str(),
            "SSH: publickey offered (probe, no signature yet)"
        );
        // Accept all probes — actual key validation happens in auth_publickey
        Ok(Auth::Accept)
    }

    async fn auth_publickey(
        &mut self,
        user: &str,
        key: &russh::keys::PublicKey,
    ) -> Result<Auth, Self::Error> {
        let fp = format!("{}", key.fingerprint(russh::keys::ssh_key::HashAlg::Sha256));

        tracing::info!(peer_ip = %self.peer_ip, %user, %fp, "SSH: auth attempt (publickey)");

        // Look up key in database
        let ssh_key = match SshKey::find_by_fingerprint(&self.pool, &fp).await {
            Ok(Some(k)) => k,
            Ok(None) => {
                tracing::info!(%fp, peer_ip = %self.peer_ip, "SSH: auth rejected — unknown key");
                self.record_this_failure(None).await;
                let outcome = self.resolve_tarpit_outcome(None).await;
                self.log_auth_failure(None, Some(&fp), None, user, "unknown key", &outcome, true)
                    .await;
                if let tarpit::TarpitOutcome::Trap { method, .. } = outcome {
                    if method == TarpitMethod::FakeShell {
                        self.fake_shell = true;
                        return Ok(Auth::Accept);
                    }
                    if method == TarpitMethod::SlowAuth {
                        tarpit::slow_auth::delay().await;
                    }
                }
                return Ok(Auth::Reject {
                    proceed_with_methods: None,
                    partial_success: false,
                });
            }
            Err(e) => {
                tracing::error!(err = %e, %fp, peer_ip = %self.peer_ip, "SSH: auth error — db error during key lookup");
                return Ok(Auth::Reject {
                    proceed_with_methods: None,
                    partial_success: false,
                });
            }
        };

        tracing::debug!(%fp, key_id = %ssh_key.id, entity_id = %ssh_key.entity_id, "SSH: key found in db");

        // Load the owning entity (before the key-expiry checks below, so a
        // stale-but-known identity's user_id is available for user-scoped
        // ban counting even on the key-expired branches).
        let entity = match Entity::find_by_id_only(&self.pool, ssh_key.entity_id).await {
            Ok(Some(e)) => e,
            Ok(None) => {
                tracing::info!(%fp, entity_id = %ssh_key.entity_id, "SSH: auth rejected — entity not found");
                self.record_this_failure(None).await;
                let outcome = self.resolve_tarpit_outcome(None).await;
                self.log_auth_failure(
                    None,
                    Some(&fp),
                    None,
                    user,
                    "entity not found",
                    &outcome,
                    true,
                )
                .await;
                if let tarpit::TarpitOutcome::Trap { method, .. } = outcome {
                    if method == TarpitMethod::FakeShell {
                        self.fake_shell = true;
                        return Ok(Auth::Accept);
                    }
                    if method == TarpitMethod::SlowAuth {
                        tarpit::slow_auth::delay().await;
                    }
                }
                return Ok(Auth::Reject {
                    proceed_with_methods: None,
                    partial_success: false,
                });
            }
            Err(e) => {
                tracing::error!(err = %e, %fp, "SSH: auth error — db error loading entity");
                return Ok(Auth::Reject {
                    proceed_with_methods: None,
                    partial_success: false,
                });
            }
        };
        let user_id = entity.user_id;

        tracing::debug!(%fp, entity_id = %entity.id, entity_name = entity.name.as_deref().unwrap_or("(unnamed)"), "SSH: entity loaded");

        // Check key expiry (soft-deleted = expired). These branches never
        // switch to fake-shell — a stale-but-known identity's genuine
        // (if expired) credential is a normal reject, not a scanner probe.
        if let Some(valid_until) = ssh_key.ts.soft_delete.deleted_at {
            if valid_until < time::OffsetDateTime::now_utc() {
                tracing::info!(%fp, key_id = %ssh_key.id, "SSH: auth rejected — key expired (deleted_at)");
                self.record_this_failure(Some(user_id)).await;
                let outcome = self.resolve_tarpit_outcome(Some(user_id)).await;
                self.log_auth_failure(
                    Some(user_id),
                    Some(&fp),
                    None,
                    user,
                    "key expired",
                    &outcome,
                    true,
                )
                .await;
                if let tarpit::TarpitOutcome::Trap {
                    method: TarpitMethod::SlowAuth,
                    ..
                } = outcome
                {
                    tarpit::slow_auth::delay().await;
                }
                return Ok(Auth::Reject {
                    proceed_with_methods: None,
                    partial_success: false,
                });
            }
        }
        // Also check ssh_key's own valid_until field
        if let Some(valid_until) = ssh_key.valid_until {
            if valid_until < time::OffsetDateTime::now_utc() {
                tracing::info!(%fp, key_id = %ssh_key.id, "SSH: auth rejected — key expired (valid_until)");
                self.record_this_failure(Some(user_id)).await;
                let outcome = self.resolve_tarpit_outcome(Some(user_id)).await;
                self.log_auth_failure(
                    Some(user_id),
                    Some(&fp),
                    None,
                    user,
                    "key expired (valid_until)",
                    &outcome,
                    true,
                )
                .await;
                if let tarpit::TarpitOutcome::Trap {
                    method: TarpitMethod::SlowAuth,
                    ..
                } = outcome
                {
                    tarpit::slow_auth::delay().await;
                }
                return Ok(Auth::Reject {
                    proceed_with_methods: None,
                    partial_success: false,
                });
            }
        }

        // Check entity expiry
        if let Some(valid_until) = entity.valid_until {
            if valid_until < time::OffsetDateTime::now_utc() {
                tracing::info!(entity_id = %entity.id, entity_name = entity.name.as_deref().unwrap_or("(unnamed)"), "SSH: auth rejected — entity expired");
                self.record_this_failure(Some(user_id)).await;
                let outcome = self.resolve_tarpit_outcome(Some(user_id)).await;
                self.log_auth_failure(
                    Some(user_id),
                    Some(&fp),
                    None,
                    user,
                    "entity expired",
                    &outcome,
                    true,
                )
                .await;
                if let tarpit::TarpitOutcome::Trap {
                    method: TarpitMethod::SlowAuth,
                    ..
                } = outcome
                {
                    tarpit::slow_auth::delay().await;
                }
                return Ok(Auth::Reject {
                    proceed_with_methods: None,
                    partial_success: false,
                });
            }
        }

        // Check IP whitelist
        if let Some(ref whitelist) = entity.ip_whitelist {
            if !ip_whitelist::evaluate(whitelist, &self.peer_ip) {
                tracing::info!(
                    entity_id = %entity.id,
                    entity_name = entity.name.as_deref().unwrap_or("(unnamed)"),
                    peer_ip = %self.peer_ip,
                    "SSH: auth rejected — IP blocked by whitelist"
                );
                self.record_this_failure(Some(user_id)).await;
                let outcome = self.resolve_tarpit_outcome(Some(user_id)).await;
                self.log_auth_failure(
                    Some(user_id),
                    Some(&fp),
                    None,
                    user,
                    "ip blocked by whitelist",
                    &outcome,
                    true,
                )
                .await;
                if let tarpit::TarpitOutcome::Trap {
                    method: TarpitMethod::SlowAuth,
                    ..
                } = outcome
                {
                    tarpit::slow_auth::delay().await;
                }
                return Ok(Auth::Reject {
                    proceed_with_methods: None,
                    partial_success: false,
                });
            }
        }

        tracing::info!(
            entity_id = %entity.id,
            entity_name = entity.name.as_deref().unwrap_or("(unnamed)"),
            peer_ip = %self.peer_ip,
            %fp,
            "SSH: auth accepted"
        );
        self.log_auth_success(&entity, &fp, user).await;
        self.entity = Some(AuthedEntity { entity, user_id });

        Ok(Auth::Accept)
    }

    // Called after successful authentication — send welcome message and set up session channel.
    async fn auth_succeeded(&mut self, session: &mut Session) -> Result<(), Self::Error> {
        if self.fake_shell {
            // No real entity to register/broadcast — just stand the channel
            // up in the background; the bogus prompt itself is sent once the
            // client actually opens a session channel (see channel_open_session).
            let handle = session.handle();
            tokio::spawn(async move {
                if let Ok(ch) = handle.channel_open_session().await {
                    tarpit::fake_shell::serve(handle, ch).await;
                }
            });
            return Ok(());
        }
        let Some(ref authed) = self.entity else {
            return Ok(());
        };
        let entity = authed.entity.clone();
        let entity_name = entity.name.as_deref().unwrap_or("(unnamed)").to_string();
        let conn_id = self.conn_id;

        let handle = session.handle();

        // Register session entry (without channel yet — filled in by
        // `channel_open_session` if/when the client itself opens one; a
        // client running `-N` never does, and that's fine — no push
        // channel is needed for a pure port-forward).
        {
            let mut reg = self.session_registry.lock().await;
            reg.insert(
                conn_id,
                SessionEntry {
                    handle: handle.clone(),
                    session_channel_id: None,
                },
            );
        }

        // Broadcast arrival to all other sessions
        let connect_msg = format!(
            "\r\n\x1b[32m📡 {} connected.\x1b[0m\r\n\r\n",
            entity_name
        );
        broadcast(&self.session_registry, &connect_msg, Some(conn_id)).await;

        Ok(())
    }

    // Accept client-initiated session channels (interactive `ssh` without -N).
    async fn channel_open_session(
        &mut self,
        channel: Channel<Msg>,
        _session: &mut Session,
    ) -> Result<bool, Self::Error> {
        if self.entity.is_none() && self.fake_shell {
            let handle = _session.handle();
            tokio::spawn(tarpit::fake_shell::serve(handle, channel));
            return Ok(true);
        }
        let Some(ref authed) = self.entity else {
            return Ok(false);
        };
        let entity_name = authed
            .entity
            .name
            .as_deref()
            .unwrap_or("(unnamed)")
            .to_string();
        let conn_id = self.conn_id;
        let ch_id = channel.id();

        // Use this client-opened channel for receiving chat input.
        // Update the registry so broadcasts reach this channel
        if let Some(entry) = self.session_registry.lock().await.get_mut(&conn_id) {
            entry.session_channel_id = Some(ch_id);
        }

        tracing::debug!(%conn_id, ch = %ch_id, "SSH: client-initiated session channel accepted");

        // Keep the channel alive in a background task
        let handle = _session.handle();
        let welcome = format!(
            "\r\n\x1b[35m✨ Welcome to tunnel2tunnel, {}! ✨\x1b[0m\r\n\
             \x1b[34mTwilight Sparkle has verified your access rules — everything checks out.\r\n\
             The portal stands ready. Your connection is now active. 📚\r\n\
             Type a message and press Enter to chat with other connected entities.\x1b[0m\r\n\r\n",
            entity_name
        );
        let _ = handle.data(ch_id, welcome.into_bytes()).await;

        // Keep the channel alive and send periodic Derpy pings
        let ping_handle = handle.clone();
        tokio::spawn(async move {
            let mut ch = channel;
            let mut interval = tokio::time::interval(Duration::from_secs(5 * 60));
            interval.tick().await; // skip first immediate tick
            loop {
                tokio::select! {
                    msg = ch.wait() => {
                        match msg {
                            None | Some(ChannelMsg::Eof) | Some(ChannelMsg::Close) => break,
                            _ => {}
                        }
                    }
                    _ = interval.tick() => {
                        let ping = format!(
                            "\r\n\x1b[33m[{}] ✉ Derpy Hooves stopped by to make sure your tunnel is still up! 🧁\x1b[0m\r\n\r\n",
                            hms_timestamp(),
                        );
                        if ping_handle.data(ch_id, ping.into_bytes()).await.is_err() {
                            break;
                        }
                    }
                }
            }
        });

        Ok(true)
    }

    // No real pty/shell/exec is provided — this server only offers the
    // welcome/chat channel and tunnel bridging — but every `want_reply`
    // channel request must still get *some* reply, or an interactive
    // (non `-N`) OpenSSH client is left waiting forever on it.
    async fn pty_request(
        &mut self,
        channel: ChannelId,
        _term: &str,
        _col_width: u32,
        _row_height: u32,
        _pix_width: u32,
        _pix_height: u32,
        _modes: &[(Pty, u32)],
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        session.channel_success(channel)?;
        Ok(())
    }

    async fn env_request(
        &mut self,
        channel: ChannelId,
        _variable_name: &str,
        _variable_value: &str,
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        session.channel_success(channel)?;
        Ok(())
    }

    async fn shell_request(
        &mut self,
        channel: ChannelId,
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        session.channel_success(channel)?;
        Ok(())
    }

    async fn window_change_request(
        &mut self,
        channel: ChannelId,
        _col_width: u32,
        _row_height: u32,
        _pix_width: u32,
        _pix_height: u32,
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        session.channel_success(channel)?;
        Ok(())
    }

    async fn exec_request(
        &mut self,
        channel: ChannelId,
        _data: &[u8],
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        session.channel_failure(channel)?;
        Ok(())
    }

    async fn subsystem_request(
        &mut self,
        channel: ChannelId,
        _name: &str,
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        session.channel_failure(channel)?;
        Ok(())
    }

    // Server registers a remote port (-R proxy_port:...)
    async fn tcpip_forward(
        &mut self,
        address: &str,
        port: &mut u32,
        session: &mut Session,
    ) -> Result<bool, Self::Error> {
        let Some(ref authed) = self.entity else {
            return Ok(false);
        };
        let entity_id = authed.entity.id;
        let entity_name = authed
            .entity
            .name
            .as_deref()
            .unwrap_or("(unnamed)")
            .to_string();
        tracing::info!(%entity_id, proxy_port = port, "server registered port");

        // Auto-create a port_configs row if none exists yet for this
        // (entity, proxy_port) — running `-R port:...` with no prior UI
        // setup should immediately produce a real, named, enabled service
        // rather than leaving it in an "unconfigured port" limbo state.
        match PortConfig::find_enabled_by_entity_and_proxy_port(&self.pool, entity_id, *port as i32)
            .await
        {
            Ok(Some(_)) => {}
            Ok(None) => {
                let name = guess_service_name(*port as i32).unwrap_or("Unnamed Service");
                let host = slugify_host(name);
                match PortConfig::create(
                    &self.pool,
                    entity_id,
                    true,
                    *port as i32,
                    *port as i32,
                    name,
                    None,
                    0,
                    &host,
                )
                .await
                {
                    Ok(pc) => {
                        tracing::info!(%entity_id, proxy_port = port, port_config_id = %pc.id, %name, "auto-created port_config for unconfigured -R forward")
                    }
                    Err(e) => {
                        tracing::warn!(err = %e, %entity_id, proxy_port = port, "failed to auto-create port_config")
                    }
                }
            }
            Err(e) => {
                tracing::warn!(err = %e, %entity_id, proxy_port = port, "failed to look up port_config for -R forward")
            }
        }

        self.server_slots
            .lock()
            .await
            .insert((entity_id, *port), (session.handle(), address.to_string()));
        let _ = self.live_update_tx.send(());
        let _ = self.live_event_tx.send(LiveEvent::PortForwardingStarted {
            entity_id,
            proxy_port: *port,
        });

        // Notify all other sessions
        let msg = format!(
            "\r\n\x1b[32m🟢 Port {} on {} is now registered.\x1b[0m\r\n\r\n",
            port, entity_name
        );
        broadcast(&self.session_registry, &msg, Some(self.conn_id)).await;

        Ok(true)
    }

    async fn cancel_tcpip_forward(
        &mut self,
        _address: &str,
        port: u32,
        _session: &mut Session,
    ) -> Result<bool, Self::Error> {
        let Some(ref authed) = self.entity else {
            return Ok(false);
        };
        let entity_name = authed
            .entity
            .name
            .as_deref()
            .unwrap_or("(unnamed)")
            .to_string();
        let entity_id = authed.entity.id;
        self.server_slots.lock().await.remove(&(entity_id, port));
        let _ = self.live_update_tx.send(());
        let _ = self.live_event_tx.send(LiveEvent::PortForwardingStopped {
            entity_id,
            proxy_port: port,
        });

        // Notify all other sessions
        let msg = format!(
            "\r\n\x1b[31m🔴 Port {} on {} is no longer available.\x1b[0m\r\n\r\n",
            port, entity_name
        );
        broadcast(&self.session_registry, &msg, Some(self.conn_id)).await;

        Ok(true)
    }

    // Client opens a direct-tcpip channel (-L local:hostname:proxy_port)
    async fn channel_open_direct_tcpip(
        &mut self,
        channel: Channel<Msg>,
        host_to_connect: &str,
        port_to_connect: u32,
        _originator_address: &str,
        _originator_port: u32,
        session: &mut Session,
    ) -> Result<bool, Self::Error> {
        let Some(ref authed) = self.entity else {
            tracing::warn!(peer_ip = %self.peer_ip, "direct-tcpip: rejected — not authenticated");
            return Ok(false);
        };
        let client_entity = authed.entity.clone();
        let client_user_id = authed.user_id;

        tracing::info!(
            client_entity = %client_entity.id,
            client_entity_name = client_entity.name.as_deref().unwrap_or("(unnamed)"),
            peer_ip = %self.peer_ip,
            host = host_to_connect,
            port = port_to_connect,
            "direct-tcpip: channel open request"
        );

        // Resolve target entity
        let target_entity_id = resolve_target_entity(&self.pool, host_to_connect).await;
        let Some(target_entity_id) = target_entity_id else {
            tracing::info!(
                host = host_to_connect,
                port = port_to_connect,
                client_entity = %client_entity.id,
                "direct-tcpip: rejected — target hostname not found (not a UUID or known alias)"
            );
            return Ok(false);
        };

        // Load target entity for user_id
        let target_entity = match Entity::find_by_id_only(&self.pool, target_entity_id).await {
            Ok(Some(e)) => e,
            Ok(None) => {
                tracing::info!(
                    target_entity = %target_entity_id,
                    client_entity = %client_entity.id,
                    "direct-tcpip: rejected — target entity not found in db"
                );
                return Ok(false);
            }
            Err(e) => {
                tracing::error!(err = %e, target_entity = %target_entity_id, "direct-tcpip: db error loading target entity");
                return Ok(false);
            }
        };

        // Step 1: the target must have an enabled port_config for this
        // proxy_port — a real config/authorization miss, distinct from
        // "server offline", so this fails fast.
        let port_config = match PortConfig::find_enabled_by_entity_and_proxy_port(
            &self.pool,
            target_entity_id,
            port_to_connect as i32,
        )
        .await
        {
            Ok(Some(pc)) => pc,
            Ok(None) => {
                tracing::warn!(
                    client_entity = %client_entity.id,
                    target_entity = %target_entity_id,
                    target_entity_name = target_entity.name.as_deref().unwrap_or("(unnamed)"),
                    proxy_port = port_to_connect,
                    "direct-tcpip: rejected — target has no enabled port_config for this port"
                );
                return Ok(false);
            }
            Err(e) => {
                tracing::error!(err = %e, target_entity = %target_entity_id, proxy_port = port_to_connect, "direct-tcpip: db error loading port_config");
                return Ok(false);
            }
        };

        // Step 2: the client must have actually opted in via a
        // port_subscriptions row, and it must currently be enabled.
        let subscription = match PortSubscription::find_by_subscriber_and_port_config(
            &self.pool,
            client_entity.id,
            port_config.id,
        )
        .await
        {
            Ok(Some(sub)) => sub,
            Ok(None) => {
                tracing::warn!(
                    client_entity = %client_entity.id,
                    target_entity = %target_entity_id,
                    port_config_id = %port_config.id,
                    proxy_port = port_to_connect,
                    "direct-tcpip: rejected — client has no subscription to this port_config"
                );
                return Ok(false);
            }
            Err(e) => {
                tracing::error!(err = %e, client_entity = %client_entity.id, port_config_id = %port_config.id, "direct-tcpip: db error loading subscription");
                return Ok(false);
            }
        };
        if !subscription.enabled {
            tracing::warn!(
                client_entity = %client_entity.id,
                target_entity = %target_entity_id,
                port_config_id = %port_config.id,
                "direct-tcpip: rejected — subscription exists but is disabled"
            );
            return Ok(false);
        }

        // Step 3: same-account access is implicit; otherwise re-verify
        // entity_access live, so revoking access also revokes function even
        // if a stale subscription row is left behind.
        if client_user_id != target_entity.user_id {
            let allowed = EntityAccess::check_access(
                &self.pool,
                target_entity_id,
                Some(port_config.id),
                client_entity.id,
                client_user_id,
                target_entity.user_id,
            )
            .await
            .unwrap_or(false);

            if !allowed {
                tracing::warn!(
                    client_entity = %client_entity.id,
                    target_entity = %target_entity_id,
                    target_entity_name = target_entity.name.as_deref().unwrap_or("(unnamed)"),
                    port_config_id = %port_config.id,
                    host = host_to_connect,
                    port = port_to_connect,
                    "direct-tcpip: rejected — access denied (entity_access revoked?)"
                );
                return Ok(false);
            }
        }

        // Step 4: server-liveness is retried, not rejected outright — a
        // subscriber whose service is correctly configured and authorized
        // but whose target server just hasn't registered yet (races on
        // startup, a server mid-restart) gets bridged the moment it
        // appears, no reconnect needed.
        //
        // Fast path first: if the target is already registered (the
        // overwhelmingly common case), do everything synchronously and
        // return `Ok(true)` only once the bridge is actually wired up —
        // exactly as before this fix. This matters, not just for latency:
        // russh sends the client its channel-open confirmation the instant
        // this function returns `Ok(true)`, and a `-L` client starts
        // writing its request the instant it sees that confirmation. If
        // confirmation went out before `self.bridges` had this channel's
        // entry, those first bytes would hit `data()` before it has
        // anywhere to forward them and be silently dropped. Keeping the
        // fast path fully synchronous sidesteps that race entirely for
        // every request that doesn't need to wait.
        let immediate_slot = self
            .server_slots
            .lock()
            .await
            .get(&(target_entity_id, port_to_connect))
            .cloned();

        if let Some((server_handle, registered_address)) = immediate_slot {
            let server_ch = match server_handle
                .channel_open_forwarded_tcpip(&registered_address, port_to_connect, "127.0.0.1", 0)
                .await
            {
                Ok(ch) => ch,
                Err(e) => {
                    tracing::error!(
                        err = ?e,
                        target_entity = %target_entity_id,
                        target_entity_name = target_entity.name.as_deref().unwrap_or("(unnamed)"),
                        proxy_port = port_to_connect,
                        "direct-tcpip: forwarded-tcpip open failed"
                    );
                    return Ok(false);
                }
            };

            let client_ch_id = channel.id();
            let server_ch_id = server_ch.id();

            self.bridges
                .lock()
                .await
                .insert(client_ch_id, (server_handle.clone(), server_ch_id));

            let bridge_id = Uuid::now_v7();
            self.active_tunnels.lock().await.insert(
                bridge_id,
                ActiveTunnelInfo {
                    client_entity_id: client_entity.id,
                    client_user_id,
                    target_entity_id,
                    port_config_id: port_config.id,
                    proxy_port: port_to_connect,
                    peer_ip: self.peer_ip.clone(),
                    since: time::OffsetDateTime::now_utc(),
                },
            );
            self.bridge_ids.lock().await.insert(client_ch_id, bridge_id);
            let _ = self.live_update_tx.send(());
            let _ = self.live_event_tx.send(LiveEvent::BridgeStarted {
                client_entity_id: client_entity.id,
                target_entity_id,
                proxy_port: port_to_connect,
            });
            let chat_msg = format!(
                "\r\n\x1b[32m[{}] 🔗 {} connected to {} on {}.\x1b[0m\r\n\r\n",
                hms_timestamp(),
                client_entity.name.as_deref().unwrap_or("(unnamed)"),
                port_config.name,
                target_entity.name.as_deref().unwrap_or("(unnamed)"),
            );
            broadcast(&self.session_registry, &chat_msg, Some(self.conn_id)).await;

            let client_handle = session.handle();
            tokio::spawn(forward_channel(server_ch, client_handle, client_ch_id));

            tracing::info!(
                client_entity = %client_entity.id,
                target_entity = %target_entity_id,
                target_entity_name = target_entity.name.as_deref().unwrap_or("(unnamed)"),
                host = host_to_connect,
                port = port_to_connect,
                client_ch = %client_ch_id,
                server_ch = %server_ch_id,
                "direct-tcpip: bridge established"
            );
            return Ok(true);
        }

        // Slow path: the target isn't registered yet. This is the case
        // that used to run its up-to-12s retry-wait loop inline inside this
        // `Handler` callback — since russh processes one SSH connection on
        // a single per-connection task (see CLAUDE.md), that blocked every
        // other channel-open request AND all data delivery on this
        // connection's already-established bridges for the duration —
        // observed in production as a second `-L` tunnel on the same `ssh`
        // invocation stalling behind an unrelated, never-coming-online
        // target. Move the wait (and the bridge setup that follows it) into
        // a spawned task and confirm `Ok(true)` immediately so this
        // connection isn't blocked. Trade-off: russh consumes this return
        // value synchronously to send the confirmation, with no API to
        // defer it, so a target that never comes online can no longer be
        // rejected before confirmation — the spawned task closes the
        // channel once the retry window lapses instead. This also reopens,
        // for this slow path only, the confirm-before-bridge race described
        // above — acceptable here since by definition nothing has
        // successfully used this specific tunnel yet.
        let handle = session.handle();
        let server_slots = self.server_slots.clone();
        let bridges = self.bridges.clone();
        let bridge_ids = self.bridge_ids.clone();
        let active_tunnels = self.active_tunnels.clone();
        let live_update_tx = self.live_update_tx.clone();
        let live_event_tx = self.live_event_tx.clone();
        let session_registry = self.session_registry.clone();
        let conn_id = self.conn_id;
        let peer_ip = self.peer_ip.clone();
        let host_to_connect = host_to_connect.to_string();

        tokio::spawn(async move {
            let client_ch_id = channel.id();

            const POLL_INTERVAL: Duration = Duration::from_millis(250);
            const MAX_WAIT: Duration = Duration::from_secs(12);
            let deadline = tokio::time::Instant::now() + MAX_WAIT;
            let server_slot = loop {
                let found = server_slots
                    .lock()
                    .await
                    .get(&(target_entity_id, port_to_connect))
                    .cloned();
                if found.is_some() {
                    break found;
                }
                if tokio::time::Instant::now() >= deadline {
                    break None;
                }
                tokio::time::sleep(POLL_INTERVAL).await;
            };

            let Some((server_handle, registered_address)) = server_slot else {
                tracing::info!(
                    target_entity = %target_entity_id,
                    target_entity_name = target_entity.name.as_deref().unwrap_or("(unnamed)"),
                    proxy_port = port_to_connect,
                    "direct-tcpip: rejected — target server never came online within the retry window"
                );
                let _ = channel.close().await;
                return;
            };

            // Ask server to open forwarded channel to its local service. The address must match
            // what the server's ssh client registered via tcpip_forward (e.g. "localhost" from
            // `-R port:...`) — OpenSSH matches incoming forwarded-tcpip requests against its
            // registered (address, port) forward table, not the client's requested hostname.
            let server_ch = match server_handle
                .channel_open_forwarded_tcpip(&registered_address, port_to_connect, "127.0.0.1", 0)
                .await
            {
                Ok(ch) => ch,
                Err(e) => {
                    tracing::error!(
                        err = ?e,
                        target_entity = %target_entity_id,
                        target_entity_name = target_entity.name.as_deref().unwrap_or("(unnamed)"),
                        proxy_port = port_to_connect,
                        "direct-tcpip: forwarded-tcpip open failed"
                    );
                    let _ = channel.close().await;
                    return;
                }
            };

            let server_ch_id = server_ch.id();

            bridges
                .lock()
                .await
                .insert(client_ch_id, (server_handle.clone(), server_ch_id));

            // Record this bridge in the aggregated active-tunnels map for the
            // live-connections dashboard, and remember which bridge id belongs
            // to this channel so it can be torn down again on close/disconnect.
            let bridge_id = Uuid::now_v7();
            active_tunnels.lock().await.insert(
                bridge_id,
                ActiveTunnelInfo {
                    client_entity_id: client_entity.id,
                    client_user_id,
                    target_entity_id,
                    port_config_id: port_config.id,
                    proxy_port: port_to_connect,
                    peer_ip,
                    since: time::OffsetDateTime::now_utc(),
                },
            );
            bridge_ids.lock().await.insert(client_ch_id, bridge_id);
            let _ = live_update_tx.send(());
            let _ = live_event_tx.send(LiveEvent::BridgeStarted {
                client_entity_id: client_entity.id,
                target_entity_id,
                proxy_port: port_to_connect,
            });
            let chat_msg = format!(
                "\r\n\x1b[32m[{}] 🔗 {} connected to {} on {}.\x1b[0m\r\n\r\n",
                hms_timestamp(),
                client_entity.name.as_deref().unwrap_or("(unnamed)"),
                port_config.name,
                target_entity.name.as_deref().unwrap_or("(unnamed)"),
            );
            broadcast(&session_registry, &chat_msg, Some(conn_id)).await;

            // Spawn task: copy server channel → client session
            tokio::spawn(forward_channel(server_ch, handle.clone(), client_ch_id));

            tracing::info!(
                client_entity = %client_entity.id,
                target_entity = %target_entity_id,
                target_entity_name = target_entity.name.as_deref().unwrap_or("(unnamed)"),
                host = host_to_connect,
                port = port_to_connect,
                client_ch = %client_ch_id,
                server_ch = %server_ch_id,
                "direct-tcpip: bridge established"
            );
        });

        Ok(true)
    }

    async fn data(
        &mut self,
        channel: ChannelId,
        data: &[u8],
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        // Bridge: forward data to the paired channel
        let state = self.bridges.lock().await.get(&channel).cloned();
        if let Some((handle, ch)) = state {
            let _ = handle.data(ch, data.to_vec()).await;
            return Ok(());
        }

        // Session channel: buffer raw keystrokes until Enter, echoing each
        // byte straight back so the sender sees what they've typed — no real
        // pty is allocated (see `pty_request`'s doc comment), so nothing
        // echoes locally on its own.
        let session_channel = self
            .session_registry
            .lock()
            .await
            .get(&self.conn_id)
            .and_then(|e| e.session_channel_id);
        if session_channel != Some(channel) || self.entity.is_none() {
            return Ok(());
        }

        for &byte in data {
            match byte {
                b'\r' | b'\n' => {
                    let _ = session.data(channel, b"\r\n".to_vec());
                    if self.chat_input.is_empty() {
                        continue;
                    }
                    let text = String::from_utf8_lossy(&self.chat_input).into_owned();
                    self.chat_input.clear();
                    let Some(ref authed) = self.entity else {
                        continue;
                    };
                    let entity_name = authed.entity.name.as_deref().unwrap_or("(unnamed)");
                    let short_id = &authed.entity.id.to_string()[..8];
                    let msg = format!(
                        "\r\n\x1b[36m[{}] {} ({}): {}\x1b[0m\r\n\r\n",
                        hms_timestamp(),
                        entity_name,
                        short_id,
                        text
                    );
                    broadcast(&self.session_registry, &msg, Some(self.conn_id)).await;
                }
                0x7f | 0x08 => {
                    if self.chat_input.pop().is_some() {
                        let _ = session.data(channel, b"\x08 \x08".to_vec());
                    }
                }
                0x03 => {
                    // Ctrl-C: graceful disconnect from the chat session.
                    let _ = session.data(channel, b"\r\n^C\r\n".to_vec());
                    let _ = session.close(channel);
                }
                0x00..=0x1f => {
                    // Ignore other control bytes (escape sequences, ...).
                }
                _ => {
                    self.chat_input.push(byte);
                    let _ = session.data(channel, vec![byte]);
                }
            }
        }

        Ok(())
    }

    async fn channel_eof(
        &mut self,
        channel: ChannelId,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        if let Some((handle, ch)) = self.bridges.lock().await.get(&channel).cloned() {
            let _ = handle.eof(ch).await;
        }
        Ok(())
    }

    async fn channel_close(
        &mut self,
        channel: ChannelId,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        if let Some((handle, ch)) = self.bridges.lock().await.remove(&channel) {
            let _ = handle.close(ch).await;
        }
        if let Some(bridge_id) = self.bridge_ids.lock().await.remove(&channel) {
            let removed = self.active_tunnels.lock().await.remove(&bridge_id);
            let _ = self.live_update_tx.send(());
            if let Some(info) = removed {
                let _ = self.live_event_tx.send(LiveEvent::BridgeStopped {
                    client_entity_id: info.client_entity_id,
                    target_entity_id: info.target_entity_id,
                    proxy_port: info.proxy_port,
                });
                let chat_msg = format!(
                    "\r\n\x1b[31m[{}] 🔌 {} disconnected from {} on {}.\x1b[0m\r\n\r\n",
                    hms_timestamp(),
                    entity_display_name(&self.pool, info.client_entity_id).await,
                    port_config_display_name(&self.pool, info.port_config_id).await,
                    entity_display_name(&self.pool, info.target_entity_id).await,
                );
                broadcast(&self.session_registry, &chat_msg, None).await;
            }
        }
        Ok(())
    }
}

impl Drop for T2tHandler {
    fn drop(&mut self) {
        match &self.entity {
            Some(authed) => tracing::info!(
                entity_id = %authed.entity.id,
                entity_name = authed.entity.name.as_deref().unwrap_or("(unnamed)"),
                peer_ip = %self.peer_ip,
                "SSH: connection closed (authenticated)"
            ),
            None => {
                tracing::info!(peer_ip = %self.peer_ip, "SSH: connection closed (unauthenticated)")
            }
        }

        // Broadcast disconnect and clean up session registry
        if let Some(ref authed) = self.entity {
            let entity_name = authed
                .entity
                .name
                .as_deref()
                .unwrap_or("(unnamed)")
                .to_string();
            let conn_id = self.conn_id;
            let registry = self.session_registry.clone();
            tokio::spawn(async move {
                registry.lock().await.remove(&conn_id);
                let msg = format!(
                    "\r\n\x1b[31m🔌 {} disconnected.\x1b[0m\r\n\r\n",
                    entity_name
                );
                broadcast(&registry, &msg, None).await;
            });
        }

        // Clean up server slots when a server entity disconnects
        if let Some(ref authed) = self.entity {
            let entity_id = authed.entity.id;
            let _ = self.live_event_tx.send(LiveEvent::EntityOffline { entity_id });
            let slots = self.server_slots.clone();
            let live_update_tx = self.live_update_tx.clone();
            let live_event_tx = self.live_event_tx.clone();
            tokio::spawn(async move {
                let mut map = slots.lock().await;
                let removed_ports: Vec<u32> = map
                    .keys()
                    .filter(|(eid, _)| *eid == entity_id)
                    .map(|(_, port)| *port)
                    .collect();
                map.retain(|(eid, _), _| *eid != entity_id);
                drop(map);
                let _ = live_update_tx.send(());
                for proxy_port in removed_ports {
                    let _ = live_event_tx.send(LiveEvent::PortForwardingStopped {
                        entity_id,
                        proxy_port,
                    });
                }
            });
        }

        // Clean up any active-tunnel entries this connection's bridges left
        // registered (a client-side `-L` connection torn down abruptly,
        // without a clean `channel_close`, would otherwise leak a "live"
        // entry forever). `bridge_ids` is now `Arc<Mutex<_>>` (bridge setup
        // can run in a spawned task, see `channel_open_direct_tcpip`), so it
        // can no longer be read synchronously here — always spawn, the async
        // task itself checks whether there's anything to do.
        {
            let active_tunnels = self.active_tunnels.clone();
            let bridge_ids = self.bridge_ids.clone();
            let live_update_tx = self.live_update_tx.clone();
            let live_event_tx = self.live_event_tx.clone();
            let session_registry = self.session_registry.clone();
            let pool = self.pool.clone();
            tokio::spawn(async move {
                let ids: Vec<Uuid> = bridge_ids.lock().await.values().copied().collect();
                if !ids.is_empty() {
                    let mut map = active_tunnels.lock().await;
                    let removed_infos: Vec<ActiveTunnelInfo> =
                        ids.iter().filter_map(|id| map.remove(id)).collect();
                    drop(map);
                    let _ = live_update_tx.send(());
                    for info in removed_infos {
                        let _ = live_event_tx.send(LiveEvent::BridgeStopped {
                            client_entity_id: info.client_entity_id,
                            target_entity_id: info.target_entity_id,
                            proxy_port: info.proxy_port,
                        });
                        let chat_msg = format!(
                            "\r\n\x1b[31m[{}] 🔌 {} disconnected from {} on {}.\x1b[0m\r\n\r\n",
                            hms_timestamp(),
                            entity_display_name(&pool, info.client_entity_id).await,
                            port_config_display_name(&pool, info.port_config_id).await,
                            entity_display_name(&pool, info.target_entity_id).await,
                        );
                        broadcast(&session_registry, &chat_msg, None).await;
                    }
                }
            });
        }

        // Mark every connection log row created for this connection as ended
        // (the successful login, if any, plus any failed/trap attempts that
        // preceded it on the same TCP connection).
        if !self.connection_log_ids.is_empty() {
            let pool = self.pool.clone();
            let log_ids = self.connection_log_ids.clone();
            let live_update_tx = self.live_update_tx.clone();
            tokio::spawn(async move {
                for log_id in log_ids {
                    if let Err(e) = ConnectionLog::set_ended(&pool, log_id).await {
                        tracing::warn!(err = %e, "failed to mark connection ended");
                    }
                }
                let _ = live_update_tx.send(());
            });
        }
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Best-effort entity display name for a chat/toast message — falls back to
/// a shortened id if the entity is unnamed or the lookup fails.
async fn entity_display_name(pool: &PgPool, entity_id: Uuid) -> String {
    Entity::find_by_id_only(pool, entity_id)
        .await
        .ok()
        .flatten()
        .and_then(|e| e.name)
        .unwrap_or_else(|| entity_id.to_string()[..8].to_string())
}

/// Best-effort service name for a chat/toast message — falls back to a
/// shortened id if the port_config was deleted or the lookup fails.
async fn port_config_display_name(pool: &PgPool, port_config_id: Uuid) -> String {
    PortConfig::find_by_id(pool, port_config_id)
        .await
        .ok()
        .flatten()
        .map(|pc| pc.name)
        .unwrap_or_else(|| port_config_id.to_string()[..8].to_string())
}

async fn resolve_target_entity(pool: &PgPool, hostname: &str) -> Option<Uuid> {
    // Try UUID first
    if let Ok(id) = hostname.parse::<Uuid>() {
        return Some(id);
    }
    // Fall back to hostname alias in entity_access
    if let Ok(Some(id)) = EntityAccess::find_entity_by_hostname(pool, hostname).await {
        return Some(id);
    }
    // Fall back to a plain entity name — only if it's unambiguous, since
    // entities.name has no uniqueness constraint.
    Entity::find_unique_id_by_name(pool, hostname)
        .await
        .unwrap_or(None)
}

async fn forward_channel(mut src: Channel<Msg>, dst: Handle, dst_ch: ChannelId) {
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
}

fn load_or_generate_host_key(path: &str, password: Option<&str>) -> Result<PrivateKey> {
    if Path::new(path).exists() {
        tracing::info!(%path, "SSH: loading host key from disk");
        let key = PrivateKey::read_openssh_file(path)
            .with_context(|| format!("failed to read SSH host key from {path}"))?;
        let key = if key.is_encrypted() {
            let pw = password.ok_or_else(|| {
                anyhow::anyhow!(
                    "SSH host key at {path} is encrypted but SSH_T2T_KEY_PASSWORD is not set"
                )
            })?;
            key.decrypt(pw)
                .with_context(|| format!("failed to decrypt SSH host key at {path}"))?
        } else {
            key
        };
        tracing::info!(
            %path,
            fingerprint = %key.public_key().fingerprint(russh::keys::ssh_key::HashAlg::Sha256),
            "SSH: host key loaded"
        );
        Ok(key)
    } else {
        tracing::info!(%path, "SSH: no host key found, generating new Ed25519 key");
        let key = PrivateKey::random(&mut UnwrapErr(SysRng), Algorithm::Ed25519)
            .context("SSH host key generation failed")?;
        if let Some(parent) = Path::new(path).parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).with_context(|| {
                    format!(
                        "failed to create directory for SSH host key: {}",
                        parent.display()
                    )
                })?;
            }
        }
        let key_to_save = if let Some(pw) = password {
            key.encrypt(&mut UnwrapErr(SysRng), pw)
                .context("failed to encrypt SSH host key")?
        } else {
            key.clone()
        };
        key_to_save
            .write_openssh_file(path, LineEnding::LF)
            .with_context(|| format!("failed to write SSH host key to {path}"))?;
        tracing::info!(
            %path,
            encrypted = password.is_some(),
            fingerprint = %key.public_key().fingerprint(russh::keys::ssh_key::HashAlg::Sha256),
            "SSH: host key generated and saved"
        );
        Ok(key)
    }
}

fn publickey_only() -> MethodSet {
    MethodSet::from(&[MethodKind::PublicKey][..])
}

/// Short `HH:MM:SS` (UTC) timestamp for chat/keepalive messages shown in the
/// SSH text channel — distinct from `chrono_like_timestamp`'s fail2ban-log
/// format.
fn hms_timestamp() -> String {
    let now = time::OffsetDateTime::now_utc();
    format!("{:02}:{:02}:{:02}", now.hour(), now.minute(), now.second())
}

fn chrono_like_timestamp() -> String {
    let now = time::OffsetDateTime::now_utc();
    let months = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let month = months[now.month() as usize - 1];
    format!(
        "{} {:2} {:02}:{:02}:{:02}",
        month,
        now.day(),
        now.hour(),
        now.minute(),
        now.second(),
    )
}

async fn append_to_file(path: &str, content: &str) -> Result<()> {
    use tokio::io::AsyncWriteExt;
    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .await?;
    file.write_all(content.as_bytes()).await?;
    Ok(())
}
