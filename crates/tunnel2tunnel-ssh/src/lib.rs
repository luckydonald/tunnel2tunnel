use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use getrandom::rand_core::UnwrapErr;
use getrandom::SysRng;
use russh::keys::{Algorithm, PrivateKey};
use russh::keys::ssh_key::LineEnding;
use russh::server::{Auth, Config, Handle, Handler, Msg, Server, Session};
use russh::{MethodKind, MethodSet};
use russh::{Channel, ChannelId, ChannelMsg};
use sqlx::PgPool;
use tokio::net::TcpListener;
use tokio::sync::Mutex;
use uuid::Uuid;

use tunnel2tunnel_core::{
    ip_whitelist,
    models::{
        connection_log::ConnectionLog,
        entity::Entity,
        entity_access::EntityAccess,
        ssh_key::SshKey,
    },
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

type ServerSlots = Arc<Mutex<HashMap<(Uuid, u32), (Handle, String)>>>;

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
                entry.session_channel_id.map(|ch| (entry.handle.clone(), ch))
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
        key.public_key().fingerprint(russh::keys::ssh_key::HashAlg::Sha256)
    ))
}

pub async fn start(config: SshConfig, pool: PgPool) -> Result<()> {
    let key = load_or_generate_host_key(
        &config.host_key_path,
        config.host_key_password.as_deref(),
    )?;

    let russh_config = Arc::new(Config {
        keys: vec![key],
        ..Config::default()
    });

    let server_slots: ServerSlots = Arc::new(Mutex::new(HashMap::new()));
    let session_registry: SessionRegistry = Arc::new(Mutex::new(HashMap::new()));
    let fail2ban = config.fail2ban_log_path.map(Arc::new);
    let tarpit_state: TarpitState = tarpit::new_state();
    let thresholds: SharedThresholds = Arc::new(Mutex::new(tarpit::ThresholdConfig::default()));

    tarpit::spawn_settings_refresher(pool.clone(), tarpit_state.clone(), thresholds.clone()).await;

    let mut server = T2tServer {
        pool: pool.clone(),
        server_slots,
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

        if let tarpit::TarpitOutcome::Trap { method: TarpitMethod::BannerDrip, source } = &pre_auth_outcome {
            let pool = server.pool.clone();
            let fail2ban = server.fail2ban.clone();
            let source = source.clone();
            tracing::info!(%peer_ip, "SSH: banner-drip tarpit engaged for new connection");
            tokio::spawn(tarpit::banner_drip::run(
                socket, peer_ip, pool, fail2ban, "ip banned", source,
            ));
            continue;
        }

        let mut handler = server.new_client(Some(addr));
        handler.tarpit_outcome = Some(pre_auth_outcome);
        let cfg = russh_config.clone();
        tokio::spawn(async move {
            match russh::server::run_stream(cfg, socket, handler).await {
                Ok(session) => {
                    let _ = session.await;
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
            session_registry: self.session_registry.clone(),
            fail2ban: self.fail2ban.clone(),
            tarpit: self.tarpit.clone(),
            thresholds: self.thresholds.clone(),
            conn_id,
            peer_ip,
            entity: None,
            bridges: HashMap::new(),
            connection_log_ids: Vec::new(),
            tarpit_outcome: None,
            fake_shell: false,
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
    session_registry: SessionRegistry,
    fail2ban: Option<Arc<String>>,
    tarpit: TarpitState,
    thresholds: SharedThresholds,
    conn_id: Uuid,
    peer_ip: String,
    entity: Option<AuthedEntity>,
    bridges: HashMap<ChannelId, (Handle, ChannelId)>,
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
}

impl T2tHandler {
    /// Lazily resolves (and caches) which tarpit outcome applies to this
    /// connection's auth attempts, once we're already inside a `Handler`
    /// callback (banner-drip is not selectable here — see `tarpit_outcome`'s
    /// doc comment).
    async fn resolve_tarpit_outcome(&mut self, user_id: Option<Uuid>) -> tarpit::TarpitOutcome {
        if self.tarpit_outcome.is_none() {
            self.tarpit_outcome = Some(
                tarpit::decide_in_auth_tarpit(&self.tarpit, &self.thresholds, &self.peer_ip, user_id)
                    .await,
            );
        }
        self.tarpit_outcome.clone().unwrap_or(tarpit::TarpitOutcome::None)
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
        counts_toward_ban: bool,
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

        if counts_toward_ban {
            let thresholds = self.thresholds.lock().await.clone();
            tarpit::record_auth_failure(&self.tarpit, &self.peer_ip, user_id, &thresholds).await;
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

    async fn log_auth_success(&mut self, entity: &Entity, fingerprint: &str, attempted_username: &str) {
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
        let outcome = self.resolve_tarpit_outcome(None).await;
        self.log_auth_failure(None, None, Some(password), user, "password auth not supported", &outcome, true)
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
        let outcome = self.resolve_tarpit_outcome(None).await;
        self.log_auth_failure(None, None, None, user, "unsupported auth method", &outcome, true)
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
        let fp = format!(
            "{}",
            key.fingerprint(russh::keys::ssh_key::HashAlg::Sha256)
        );

        tracing::info!(peer_ip = %self.peer_ip, %user, %fp, "SSH: auth attempt (publickey)");

        // Look up key in database
        let ssh_key = match SshKey::find_by_fingerprint(&self.pool, &fp).await {
            Ok(Some(k)) => k,
            Ok(None) => {
                tracing::info!(%fp, peer_ip = %self.peer_ip, "SSH: auth rejected — unknown key");
                let outcome = self.resolve_tarpit_outcome(None).await;
                self.log_auth_failure(None, Some(&fp), None, user, "unknown key", &outcome, true).await;
                if let tarpit::TarpitOutcome::Trap { method, .. } = outcome {
                    if method == TarpitMethod::FakeShell {
                        self.fake_shell = true;
                        return Ok(Auth::Accept);
                    }
                    if method == TarpitMethod::SlowAuth {
                        tarpit::slow_auth::delay().await;
                    }
                }
                return Ok(Auth::Reject { proceed_with_methods: None, partial_success: false });
            }
            Err(e) => {
                tracing::error!(err = %e, %fp, peer_ip = %self.peer_ip, "SSH: auth error — db error during key lookup");
                return Ok(Auth::Reject { proceed_with_methods: None, partial_success: false });
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
                let outcome = self.resolve_tarpit_outcome(None).await;
                self.log_auth_failure(None, Some(&fp), None, user, "entity not found", &outcome, true).await;
                if let tarpit::TarpitOutcome::Trap { method, .. } = outcome {
                    if method == TarpitMethod::FakeShell {
                        self.fake_shell = true;
                        return Ok(Auth::Accept);
                    }
                    if method == TarpitMethod::SlowAuth {
                        tarpit::slow_auth::delay().await;
                    }
                }
                return Ok(Auth::Reject { proceed_with_methods: None, partial_success: false });
            }
            Err(e) => {
                tracing::error!(err = %e, %fp, "SSH: auth error — db error loading entity");
                return Ok(Auth::Reject { proceed_with_methods: None, partial_success: false });
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
                let outcome = self.resolve_tarpit_outcome(Some(user_id)).await;
                self.log_auth_failure(Some(user_id), Some(&fp), None, user, "key expired", &outcome, true).await;
                if let tarpit::TarpitOutcome::Trap { method: TarpitMethod::SlowAuth, .. } = outcome {
                    tarpit::slow_auth::delay().await;
                }
                return Ok(Auth::Reject { proceed_with_methods: None, partial_success: false });
            }
        }
        // Also check ssh_key's own valid_until field
        if let Some(valid_until) = ssh_key.valid_until {
            if valid_until < time::OffsetDateTime::now_utc() {
                tracing::info!(%fp, key_id = %ssh_key.id, "SSH: auth rejected — key expired (valid_until)");
                let outcome = self.resolve_tarpit_outcome(Some(user_id)).await;
                self.log_auth_failure(Some(user_id), Some(&fp), None, user, "key expired (valid_until)", &outcome, true).await;
                if let tarpit::TarpitOutcome::Trap { method: TarpitMethod::SlowAuth, .. } = outcome {
                    tarpit::slow_auth::delay().await;
                }
                return Ok(Auth::Reject { proceed_with_methods: None, partial_success: false });
            }
        }

        // Check entity expiry
        if let Some(valid_until) = entity.valid_until {
            if valid_until < time::OffsetDateTime::now_utc() {
                tracing::info!(entity_id = %entity.id, entity_name = entity.name.as_deref().unwrap_or("(unnamed)"), "SSH: auth rejected — entity expired");
                let outcome = self.resolve_tarpit_outcome(Some(user_id)).await;
                self.log_auth_failure(Some(user_id), Some(&fp), None, user, "entity expired", &outcome, true).await;
                if let tarpit::TarpitOutcome::Trap { method: TarpitMethod::SlowAuth, .. } = outcome {
                    tarpit::slow_auth::delay().await;
                }
                return Ok(Auth::Reject { proceed_with_methods: None, partial_success: false });
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
                let outcome = self.resolve_tarpit_outcome(Some(user_id)).await;
                self.log_auth_failure(Some(user_id), Some(&fp), None, user, "ip blocked by whitelist", &outcome, true).await;
                if let tarpit::TarpitOutcome::Trap { method: TarpitMethod::SlowAuth, .. } = outcome {
                    tarpit::slow_auth::delay().await;
                }
                return Ok(Auth::Reject { proceed_with_methods: None, partial_success: false });
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
        let Some(ref authed) = self.entity else { return Ok(()); };
        let entity = authed.entity.clone();
        let entity_name = entity.name.as_deref().unwrap_or("(unnamed)").to_string();
        let entity_type = entity.entity_type.clone();
        let conn_id = self.conn_id;

        let handle = session.handle();

        // Register session entry (without channel yet — filled in below if open succeeds)
        {
            let mut reg = self.session_registry.lock().await;
            reg.insert(conn_id, SessionEntry {
                handle: handle.clone(),
                session_channel_id: None,
            });
        }

        // Open a server-initiated session channel to the client for push messages, in the
        // background. `channel_open_session()` awaits a confirmation that round-trips
        // through this same connection's single-task event loop — awaiting it inline here
        // (still inside that loop's packet dispatch) would deadlock permanently, since the
        // loop can never get back around to servicing its own request.
        let registry = self.session_registry.clone();
        let welcome_entity_name = entity_name.clone();
        let welcome_entity_type = entity_type.clone();
        tokio::spawn(async move {
            match handle.channel_open_session().await {
                Ok(ch) => {
                    let ch_id = ch.id();

                    // Update registry entry with channel id
                    if let Some(entry) = registry.lock().await.get_mut(&conn_id) {
                        entry.session_channel_id = Some(ch_id);
                    }

                    // Send welcome message
                    let welcome = format!(
                        "\r\n\x1b[35m✨ Welcome to tunnel2tunnel, {}! ✨\x1b[0m\r\n\
                         \x1b[34mTwilight Sparkle has verified your access rules — everything checks out.\r\n\
                         The portal stands ready. Your {} connection is now active. 📚\x1b[0m\r\n\r\n",
                        welcome_entity_name, welcome_entity_type
                    );
                    let _ = handle.data(ch_id, welcome.into_bytes()).await;

                    // Spawn task: keep channel alive and send periodic Derpy pings
                    let ping_handle = handle.clone();
                    tokio::spawn(async move {
                        let mut channel = ch;
                        let mut interval = tokio::time::interval(Duration::from_secs(5 * 60));
                        interval.tick().await; // skip first immediate tick
                        loop {
                            tokio::select! {
                                msg = channel.wait() => {
                                    match msg {
                                        None | Some(ChannelMsg::Eof) | Some(ChannelMsg::Close) => break,
                                        _ => {}
                                    }
                                }
                                _ = interval.tick() => {
                                    let ping = "\r\n\x1b[33m✉ Derpy Hooves stopped by to make sure your tunnel is still up! 🧁\x1b[0m\r\n\r\n";
                                    if ping_handle.data(ch_id, ping.as_bytes().to_vec()).await.is_err() {
                                        break;
                                    }
                                }
                            }
                        }
                    });

                    tracing::debug!(%conn_id, ch = %ch_id, "SSH: session channel opened for push messages");
                }
                Err(e) => {
                    tracing::debug!(%conn_id, err = %e, "SSH: could not open session channel (client may not support it)");
                }
            }
        });

        // Broadcast arrival to all other sessions
        let connect_msg = format!(
            "\r\n\x1b[32m📡 {} ({}) connected.\x1b[0m\r\n\r\n",
            entity_name, entity_type
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
        let Some(ref authed) = self.entity else { return Ok(false); };
        let entity_name = authed.entity.name.as_deref().unwrap_or("(unnamed)").to_string();
        let entity_type = authed.entity.entity_type.clone();
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
             The portal stands ready. Your {} connection is now active. 📚\r\n\
             Type a message and press Enter to chat with other connected entities.\x1b[0m\r\n\r\n",
            entity_name, entity_type
        );
        let _ = handle.data(ch_id, welcome.into_bytes()).await;

        tokio::spawn(async move {
            let mut ch = channel;
            loop {
                match ch.wait().await {
                    None | Some(ChannelMsg::Eof) | Some(ChannelMsg::Close) => break,
                    _ => {}
                }
            }
        });

        Ok(true)
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
        let entity_name = authed.entity.name.as_deref().unwrap_or("(unnamed)").to_string();
        tracing::info!(%entity_id, proxy_port = port, "server registered port");
        self.server_slots
            .lock()
            .await
            .insert((entity_id, *port), (session.handle(), address.to_string()));

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
        let entity_name = authed.entity.name.as_deref().unwrap_or("(unnamed)").to_string();
        self.server_slots
            .lock()
            .await
            .remove(&(authed.entity.id, port));

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

        // Check entity_access
        let allowed = EntityAccess::check_access(
            &self.pool,
            target_entity_id,
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
                host = host_to_connect,
                port = port_to_connect,
                "direct-tcpip: rejected — access denied"
            );
            return Ok(false);
        }

        // Find server handle for this entity + port
        let server_slot = self
            .server_slots
            .lock()
            .await
            .get(&(target_entity_id, port_to_connect))
            .cloned();

        let Some((server_handle, registered_address)) = server_slot else {
            tracing::info!(
                target_entity = %target_entity_id,
                target_entity_name = target_entity.name.as_deref().unwrap_or("(unnamed)"),
                proxy_port = port_to_connect,
                "direct-tcpip: rejected — target server has no registered port (server not connected?)"
            );
            return Ok(false);
        };

        // Ask server to open forwarded channel to its local service. The address must match
        // what the server's ssh client registered via tcpip_forward (e.g. "localhost" from
        // `-R port:...`) — OpenSSH matches incoming forwarded-tcpip requests against its
        // registered (address, port) forward table, not the client's requested hostname.
        let server_ch = server_handle
            .channel_open_forwarded_tcpip(
                &registered_address,
                port_to_connect,
                "127.0.0.1",
                0,
            )
            .await
            .map_err(|e| anyhow::anyhow!("forwarded-tcpip open failed: {e:?}"))?;

        let client_ch_id = channel.id();
        let server_ch_id = server_ch.id();

        self.bridges
            .insert(client_ch_id, (server_handle.clone(), server_ch_id));

        // Spawn task: copy server channel → client session
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
        Ok(true)
    }

    async fn data(
        &mut self,
        channel: ChannelId,
        data: &[u8],
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        // Bridge: forward data to the paired channel
        let state = self.bridges.get(&channel).cloned();
        if let Some((handle, ch)) = state {
            let _ = handle.data(ch, data.to_vec()).await;
            return Ok(());
        }

        // Session channel: relay as chat to all other connected entities
        let session_channel = self
            .session_registry
            .lock()
            .await
            .get(&self.conn_id)
            .and_then(|e| e.session_channel_id);
        if session_channel == Some(channel) {
            if let Some(ref authed) = self.entity {
                let text = String::from_utf8_lossy(data);
                let text = text.trim_end_matches(['\r', '\n']);
                if !text.is_empty() {
                    let entity_name = authed.entity.name.as_deref().unwrap_or("(unnamed)");
                    let short_id = &authed.entity.id.to_string()[..8];
                    let msg = format!(
                        "\r\n\x1b[36m{} ({}): {}\x1b[0m\r\n\r\n",
                        entity_name, short_id, text
                    );
                    broadcast(&self.session_registry, &msg, Some(self.conn_id)).await;
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
        if let Some((handle, ch)) = self.bridges.get(&channel).cloned() {
            let _ = handle.eof(ch).await;
        }
        Ok(())
    }

    async fn channel_close(
        &mut self,
        channel: ChannelId,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        if let Some((handle, ch)) = self.bridges.remove(&channel) {
            let _ = handle.close(ch).await;
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
            None => tracing::info!(peer_ip = %self.peer_ip, "SSH: connection closed (unauthenticated)"),
        }

        // Broadcast disconnect and clean up session registry
        if let Some(ref authed) = self.entity {
            let entity_name = authed.entity.name.as_deref().unwrap_or("(unnamed)").to_string();
            let entity_type = authed.entity.entity_type.clone();
            let conn_id = self.conn_id;
            let registry = self.session_registry.clone();
            tokio::spawn(async move {
                registry.lock().await.remove(&conn_id);
                let msg = format!(
                    "\r\n\x1b[31m🔌 {} ({}) disconnected.\x1b[0m\r\n\r\n",
                    entity_name, entity_type
                );
                broadcast(&registry, &msg, None).await;
            });
        }

        // Clean up server slots when a server entity disconnects
        if let Some(ref authed) = self.entity {
            let entity_id = authed.entity.id;
            let slots = self.server_slots.clone();
            tokio::spawn(async move {
                slots
                    .lock()
                    .await
                    .retain(|(eid, _), _| *eid != entity_id);
            });
        }

        // Mark every connection log row created for this connection as ended
        // (the successful login, if any, plus any failed/trap attempts that
        // preceded it on the same TCP connection).
        if !self.connection_log_ids.is_empty() {
            let pool = self.pool.clone();
            let log_ids = self.connection_log_ids.clone();
            tokio::spawn(async move {
                for log_id in log_ids {
                    if let Err(e) = ConnectionLog::set_ended(&pool, log_id).await {
                        tracing::warn!(err = %e, "failed to mark connection ended");
                    }
                }
            });
        }
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

async fn resolve_target_entity(pool: &PgPool, hostname: &str) -> Option<Uuid> {
    // Try UUID first
    if let Ok(id) = hostname.parse::<Uuid>() {
        return Some(id);
    }
    // Fall back to hostname alias in entity_access
    EntityAccess::find_entity_by_hostname(pool, hostname)
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
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("failed to create directory for SSH host key: {}", parent.display()))?;
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

fn chrono_like_timestamp() -> String {
    let now = time::OffsetDateTime::now_utc();
    let months = ["Jan","Feb","Mar","Apr","May","Jun","Jul","Aug","Sep","Oct","Nov","Dec"];
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
