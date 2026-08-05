//! Unified realtime push route for the live-connections dashboard,
//! per-entity view, and admin view — a single WebSocket
//! (`/api/live-connections/ws`) carrying one shape
//! (`live_connections::EntityLiveSnapshot`), scoped per-connection to either
//! the viewer's own entities (`Scope::Mine`, the default) or every entity
//! system-wide (`Scope::All`, admin-only), switched at runtime via a
//! `set_scope` command frame sent back over the same socket.
//!
//! Every push carries the full current snapshot plus an optional `reason`
//! naming *why* this particular push happened (`LiveEvent`, emitted at the
//! SSH-side mutation site where the old/new state is already known — see
//! `tunnel2tunnel_ssh::LiveEvent`). `reason: None` (initial connect / the
//! fallback resync interval / a lagged event receiver / a scope switch)
//! means "just a resync," and the frontend only ever treats a present
//! `reason` as toast-worthy for its own entities, independent of scope.

use std::collections::HashSet;
use std::time::Duration;

use axum::{
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    extract::State,
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use tunnel2tunnel_core::models::user::User;
use tunnel2tunnel_ssh::LiveEvent;
use uuid::Uuid;

use crate::{
    extractors::AuthUser,
    routes::live_connections::{build_live_connections, EntityLiveSnapshot, Scope},
    AppState,
};

/// Snapshots are rebuilt on this cadence even without a `live_event_tx` fire,
/// as a fallback for state changes the notifier doesn't cover (e.g. a
/// `port_subscriptions` row toggled via the HTTP API rather than a live SSH
/// state transition).
const FALLBACK_RESYNC_INTERVAL: Duration = Duration::from_secs(20);

#[derive(Serialize)]
struct WithReason {
    data: Vec<EntityLiveSnapshot>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<LiveEvent>,
}

#[derive(Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
enum ClientCommand {
    SetScope { scope: ClientScope },
}

#[derive(Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum ClientScope {
    Mine,
    All,
}

impl From<ClientScope> for Scope {
    fn from(s: ClientScope) -> Self {
        match s {
            ClientScope::Mine => Scope::Mine,
            ClientScope::All => Scope::All,
        }
    }
}

pub async fn live_connections_ws(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| async move { run_loop(socket, state, user).await })
}

/// Rebuilds the snapshot for the connection's current `scope`, refreshes
/// `my_ids` (used to decide whether a `Scope::Mine` connection cares about a
/// given `LiveEvent`), and sends it with the given `reason`. Returns `false`
/// if the build failed or the socket is gone — callers treat that as "stop
/// the loop."
async fn rebuild_and_send(
    who: &str,
    socket: &mut WebSocket,
    state: &AppState,
    user_id: Uuid,
    scope: Scope,
    reason: Option<LiveEvent>,
    my_ids: &mut HashSet<Uuid>,
) -> bool {
    let data = match build_live_connections(state, user_id, scope).await {
        Ok(d) => d,
        Err(e) => {
            tracing::error!(err = %e, %who, "live_ws: failed to build snapshot");
            return false;
        }
    };
    *my_ids = data
        .iter()
        .filter(|s| s.mine)
        .map(|s| s.entity_id)
        .collect();
    send_json(who, socket, &WithReason { data, reason }).await
}

/// Serializes `payload` and sends it as a WS text message. Logs both
/// outcomes — success (so server logs show what a connection is actually
/// doing, not just its failures) and failure — rather than propagating,
/// since a send failure just means "give up on this socket"; the caller
/// checks the returned bool to decide that.
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
            tracing::debug!(%who, "live_ws: sent snapshot");
            true
        }
        Err(e) => {
            tracing::warn!(err = %e, %who, "live_ws: failed to send to socket");
            false
        }
    }
}

async fn run_loop(mut socket: WebSocket, state: AppState, user: User) {
    let who = format!("user:{}", user.id);
    let mut scope = Scope::Mine;
    let mut rx = state.live_event_tx.subscribe();
    let mut interval = tokio::time::interval(FALLBACK_RESYNC_INTERVAL);
    interval.tick().await; // skip the immediate first tick — initial send below instead
    let mut my_ids: HashSet<Uuid> = HashSet::new();

    if !rebuild_and_send(&who, &mut socket, &state, user.id, scope, None, &mut my_ids).await {
        return;
    }

    loop {
        tokio::select! {
            _ = interval.tick() => {
                if !rebuild_and_send(&who, &mut socket, &state, user.id, scope, None, &mut my_ids).await {
                    return;
                }
            }
            recv = rx.recv() => {
                match recv {
                    Ok(ev) => {
                        let relevant = match scope {
                            Scope::Mine => ev.entity_ids().iter().any(|id| my_ids.contains(id)),
                            Scope::All => true,
                        };
                        if relevant
                            && !rebuild_and_send(&who, &mut socket, &state, user.id, scope, Some(ev), &mut my_ids).await
                        {
                            return;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        tracing::debug!(%who, "live_ws: event receiver lagged, resyncing anyway");
                        if !rebuild_and_send(&who, &mut socket, &state, user.id, scope, None, &mut my_ids).await {
                            return;
                        }
                    }
                    Err(broadcast::error::RecvError::Closed) => return,
                }
            }
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        match serde_json::from_str::<ClientCommand>(&text) {
                            Ok(ClientCommand::SetScope { scope: requested }) => {
                                let requested: Scope = requested.into();
                                if requested == Scope::All && !user.is_admin {
                                    tracing::warn!(%who, "live_ws: non-admin requested scope=all, ignoring");
                                } else {
                                    // Always rebuild + resend, even if `requested == scope` —
                                    // the frontend also uses re-sending its current scope as a
                                    // generic "resync now" request (e.g. right after an HTTP
                                    // action like subscribe/unsubscribe that the SSH-side
                                    // `live_event_tx` doesn't cover, rather than waiting out the
                                    // fallback interval).
                                    scope = requested;
                                    if !rebuild_and_send(&who, &mut socket, &state, user.id, scope, None, &mut my_ids).await {
                                        return;
                                    }
                                }
                            }
                            Err(e) => {
                                tracing::debug!(err = %e, %who, %text, "live_ws: unrecognized client command");
                            }
                        }
                    }
                    Some(Ok(_)) => {}
                    Some(Err(e)) => {
                        tracing::debug!(err = %e, %who, "live_ws: socket recv error");
                        return;
                    }
                    None => {
                        tracing::debug!(%who, "live_ws: socket closed by client");
                        return;
                    }
                }
            }
        }
    }
}
