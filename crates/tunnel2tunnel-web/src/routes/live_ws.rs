//! Realtime push variants of the `routes::live_connections` HTTP routes.
//! Each handler upgrades to a WebSocket, sends an initial snapshot, then
//! resends the (rebuilt) snapshot whenever `AppState::live_update_tx` fires
//! or a fallback interval ticks — see the module doc comment on
//! `tunnel2tunnel_ssh::LiveUpdateTx` for exactly which state transitions
//! notify it, and `routes::live_connections`'s module doc comment for the
//! `live`/`remote_status` semantics of the payloads themselves.

use std::time::Duration;

use axum::{
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    extract::{Path, State},
    response::IntoResponse,
};
use serde::Serialize;
use uuid::Uuid;

use crate::{
    extractors::{AdminUser, AuthUser},
    routes::{
        entities::require_owner,
        live_connections::{
            build_admin_live_connections, build_entity_live_connections, build_entity_status,
            build_my_live_connections,
        },
    },
    AppState,
};

/// Snapshots are rebuilt on this cadence even without a `live_update_tx` fire,
/// as a fallback for state changes the notifier doesn't cover (e.g. a
/// `port_subscriptions` row toggled via the HTTP API rather than a live SSH
/// state transition) — see the plan's rationale in `tunnel2tunnel-ssh/src/lib.rs`.
const FALLBACK_RESYNC_INTERVAL: Duration = Duration::from_secs(20);

/// Serializes `payload` and sends it as a WS text message. Logs both
/// outcomes — success (so server logs show what a connection is actually
/// doing, not just its failures) and failure, including the payload that
/// failed to send (small, bounded snapshots — safe to log in full) — rather
/// than propagating, since a send failure just means "give up on this
/// socket"; the caller checks the returned bool to decide that.
async fn send_json<T: Serialize>(who: &str, socket: &mut WebSocket, payload: &T) -> bool {
    let text = match serde_json::to_string(payload) {
        Ok(t) => t,
        Err(e) => {
            tracing::error!(err = %e, %who, "live_ws: failed to serialize payload");
            return false;
        }
    };
    match socket.send(Message::Text(text.clone().into())).await {
        Ok(()) => {
            tracing::debug!(%who, payload = %text, "live_ws: sent snapshot");
            true
        }
        Err(e) => {
            tracing::warn!(err = %e, %who, payload = %text, "live_ws: failed to send to socket");
            false
        }
    }
}

#[derive(Serialize)]
struct EntityLiveMessage<T> {
    entity_online: bool,
    #[serde(with = "time::serde::rfc3339::option")]
    entity_last_disconnected_at: Option<time::OffsetDateTime>,
    #[serde(flatten)]
    live: T,
}

pub async fn entity_live_connections_ws(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Path(entity_id): Path<Uuid>,
    ws: WebSocketUpgrade,
) -> axum::response::Response {
    match require_owner(&state.db, entity_id, user.id).await {
        Ok(_) => {}
        Err(e) => return e.into_response(),
    }
    ws.on_upgrade(move |socket| async move {
        let who = format!("entity:{entity_id}");
        run_loop(&who, socket, state, move |state: AppState| async move {
            let live = build_entity_live_connections(&state, entity_id).await?;
            let (entity_online, entity_last_disconnected_at) =
                build_entity_status(&state, entity_id).await?;
            Ok(EntityLiveMessage {
                entity_online,
                entity_last_disconnected_at,
                live,
            })
        })
        .await;
    })
}

pub async fn my_live_connections_ws(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| async move {
        let who = format!("me:{}", user.id);
        run_loop(&who, socket, state, move |state: AppState| async move {
            build_my_live_connections(&state, user.id).await
        })
        .await;
    })
}

pub async fn admin_live_connections_ws(
    AdminUser(_admin): AdminUser,
    State(state): State<AppState>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| async move {
        let who = "admin".to_string();
        run_loop(&who, socket, state, move |state: AppState| async move {
            build_admin_live_connections(&state).await
        })
        .await;
    })
}

/// Shared send/resync loop: sends one snapshot immediately, then rebuilds
/// and resends whenever `live_update_tx` fires or the fallback interval
/// ticks, until the socket closes or `build` errors.
async fn run_loop<T, F, Fut>(who: &str, mut socket: WebSocket, state: AppState, build: F)
where
    T: Serialize,
    F: Fn(AppState) -> Fut,
    Fut: std::future::Future<Output = Result<T, crate::WebError>>,
{
    let mut rx = state.live_update_tx.subscribe();
    let mut interval = tokio::time::interval(FALLBACK_RESYNC_INTERVAL);
    interval.tick().await; // skip the immediate first tick — we send below instead

    loop {
        match build(state.clone()).await {
            Ok(payload) => {
                if !send_json(who, &mut socket, &payload).await {
                    return;
                }
            }
            Err(e) => {
                tracing::error!(err = %e, %who, "live_ws: failed to build snapshot");
                return;
            }
        }

        tokio::select! {
            _ = interval.tick() => {}
            recv = rx.recv() => {
                // `Lagged` just means we missed some notifications while busy
                // sending/building — resync unconditionally either way.
                if recv.is_err() {
                    tracing::debug!(%who, "live_ws: notifier lagged, resyncing anyway");
                }
            }
            msg = socket.recv() => {
                // Client closed the socket (or sent something we don't act
                // on) — either way, once the socket yields `None` it's gone.
                if msg.is_none() {
                    tracing::debug!(%who, "live_ws: socket closed by client");
                    return;
                }
            }
        }
    }
}
