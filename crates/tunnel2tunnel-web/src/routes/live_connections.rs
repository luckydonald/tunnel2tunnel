//! Live connections dashboard — read-only views over the in-memory
//! `ServerSlots`/`ActiveTunnels` maps shared with the SSH server (see
//! `AppState` in `crates/tunnel2tunnel-web/src/lib.rs`), joined with the
//! `port_configs`/`port_subscriptions` tables for display context.
//!
//! Status is split into two channels that now share the same 4-state
//! semantics on both the owner (service) side and the subscriber
//! (subscription) side:
//!   - `live` (dot) — is traffic actually flowing right now for *this row*?
//!     For a service (owner) row this aggregates over all current
//!     subscribers: `true` iff at least one subscriber has a live bridge.
//!     For a subscription row it's this subscriber's own bridge state.
//!   - `remote_status` (ring) — always populated (services included).
//!     For a subscription row it reflects the *owner* entity's SSH
//!     session/port; for a service row it reflects *this entity's own*
//!     session/port (from its own point of view). Four states:
//!       - `offline`       — not SSH-connected (or the row is disabled).
//!       - `not_forwarded` — SSH-connected, but hasn't forwarded this port yet.
//!       - `idle`          — port is forwarded/routable, but no active bridge.
//!       - `active`        — an active bridge exists right now (matches `live`).
//!     A disabled subscription/service is always `live = false, remote_status = offline`.

use std::collections::HashSet;

use axum::{
    extract::{Path, State},
    Json,
};
use serde::Serialize;
use time::OffsetDateTime;
use tunnel2tunnel_core::models::{
    connection_log::ConnectionLog, entity::Entity, port_config::PortConfig,
    port_subscription::PortSubscription, user::User,
};
use uuid::Uuid;

use crate::{
    extractors::{AdminUser, AuthUser},
    routes::entities::require_owner,
    AppState, WebError,
};

// ── Shared response fragments ─────────────────────────────────────────────────

#[derive(Serialize)]
pub struct AccountRef {
    pub user_id: Uuid,
    pub username: String,
}

#[derive(Serialize)]
pub struct EntityRef {
    pub id: Uuid,
    pub name: Option<String>,
}

// ── Per-entity response ────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct SubscriberInfo {
    pub entity: EntityRef,
    pub account: AccountRef,
    pub peer_ip: String,
    #[serde(with = "time::serde::rfc3339")]
    pub connected_since: OffsetDateTime,
}

/// Ring status — named for the state, not the color the frontend maps it
/// to. See the module doc comment for the full semantics.
#[derive(Serialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum RemoteStatus {
    Offline,
    NotForwarded,
    Idle,
    Active,
}

#[derive(Serialize)]
pub struct ServiceLiveStatus {
    pub port_config_id: Uuid,
    pub service_name: String,
    pub proxy_port: i32,
    /// Aggregated over current subscribers — see module doc comment.
    pub live: bool,
    /// This entity's own SSH-session/port state — see module doc comment.
    pub remote_status: Option<RemoteStatus>,
    pub subscribers: Vec<SubscriberInfo>,
}

#[derive(Serialize)]
pub struct SubscriptionLiveStatus {
    pub subscription_id: Uuid,
    pub port_config_id: Uuid,
    pub owner: EntityRef,
    pub service_name: String,
    pub proxy_port: i32,
    pub subscriber_local_port: i32,
    pub enabled: bool,
    pub live: bool,
    pub remote_status: Option<RemoteStatus>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub connected_since: Option<OffsetDateTime>,
}

#[derive(Serialize)]
pub struct EntityLiveConnectionsResponse {
    /// This entity's own `port_configs` ("my services"), each with its live
    /// status and currently-bridged subscribers.
    pub services: Vec<ServiceLiveStatus>,
    /// This entity's own `port_subscriptions` ("my subscriptions"), each
    /// with its own live status.
    pub subscriptions: Vec<SubscriptionLiveStatus>,
}

pub async fn list_entity_live_connections(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Path(entity_id): Path<Uuid>,
) -> Result<Json<EntityLiveConnectionsResponse>, WebError> {
    require_owner(&state.db, entity_id, user.id).await?;
    Ok(Json(build_entity_live_connections(&state, entity_id).await?))
}

/// Reusable snapshot builder — shared by the HTTP route above and the
/// `live-connections/ws` WebSocket route (`routes::live_ws`). Callers are
/// responsible for their own auth check (`require_owner`/`AdminUser`) before
/// calling this.
pub async fn build_entity_live_connections(
    state: &AppState,
    entity_id: Uuid,
) -> Result<EntityLiveConnectionsResponse, WebError> {
    // Snapshot the in-memory maps once up front — cheap (small, short-lived
    // locks) and avoids re-locking per row below.
    let live_slots: HashSet<(Uuid, u32)> = state
        .server_slots
        .lock()
        .await
        .keys()
        .copied()
        .collect();
    let active_tunnels: Vec<tunnel2tunnel_ssh::ActiveTunnelInfo> =
        state.active_tunnels.lock().await.values().cloned().collect();
    let (entity_online, _) = ConnectionLog::entity_status(&state.db, entity_id)
        .await
        .map_err(WebError::Core)?;

    // ── My services ────────────────────────────────────────────────────────
    let owned_ports = PortConfig::list_for_entity(&state.db, entity_id).await?;
    let mut services = Vec::with_capacity(owned_ports.len());
    for pc in owned_ports {
        let mut live = false;
        let mut subscribers = Vec::new();
        for info in active_tunnels
            .iter()
            .filter(|info| info.target_entity_id == entity_id && info.proxy_port == pc.proxy_port as u32)
        {
            live = true;
            let sub_entity = Entity::find_by_id_only(&state.db, info.client_entity_id)
                .await
                .map_err(WebError::Core)?;
            let sub_user = User::find_by_id(&state.db, info.client_user_id)
                .await
                .map_err(WebError::Core)?;
            subscribers.push(SubscriberInfo {
                entity: EntityRef {
                    id: info.client_entity_id,
                    name: sub_entity.and_then(|e| e.name),
                },
                account: AccountRef {
                    user_id: info.client_user_id,
                    username: sub_user.map(|u| u.username).unwrap_or_default(),
                },
                peer_ip: info.peer_ip.clone(),
                connected_since: info.since,
            });
        }
        let port_forwarded = live_slots.contains(&(entity_id, pc.proxy_port as u32));
        let remote_status = remote_status_for(pc.enabled, entity_online, port_forwarded, live);
        services.push(ServiceLiveStatus {
            port_config_id: pc.id,
            service_name: pc.name,
            proxy_port: pc.proxy_port,
            live,
            remote_status: Some(remote_status),
            subscribers,
        });
    }

    // ── My subscriptions ───────────────────────────────────────────────────
    let subs = PortSubscription::list_for_subscriber_with_context(&state.db, entity_id).await?;
    let owner_ids: Vec<Uuid> = subs.iter().map(|s| s.port_config.entity_id).collect();
    let owner_statuses = ConnectionLog::entity_statuses(&state.db, &owner_ids)
        .await
        .map_err(WebError::Core)?;
    let mut subscriptions = Vec::with_capacity(subs.len());
    for s in subs {
        let bridge = active_tunnels.iter().find(|info| {
            info.client_entity_id == entity_id && info.port_config_id == s.port_config.id
        });
        let owner_online = owner_statuses
            .get(&s.port_config.entity_id)
            .map(|(online, _)| *online)
            .unwrap_or(false);
        let owner_port_live =
            live_slots.contains(&(s.port_config.entity_id, s.port_config.proxy_port as u32));
        let live = subscription_live(s.subscription.enabled, bridge.is_some());
        let remote_status =
            remote_status_for(s.subscription.enabled, owner_online, owner_port_live, bridge.is_some());
        subscriptions.push(SubscriptionLiveStatus {
            subscription_id: s.subscription.id,
            port_config_id: s.port_config.id,
            owner: EntityRef {
                id: s.port_config.entity_id,
                name: s.owner_entity_name,
            },
            service_name: s.port_config.name,
            proxy_port: s.port_config.proxy_port,
            subscriber_local_port: s.subscription.subscriber_local_port,
            enabled: s.subscription.enabled,
            live,
            remote_status: Some(remote_status),
            connected_since: bridge.map(|b| b.since),
        });
    }

    Ok(EntityLiveConnectionsResponse {
        services,
        subscriptions,
    })
}

/// Reusable single-entity online-status lookup — same source
/// (`ConnectionLog::entity_status`) that `routes::entities::get_entity` uses
/// for `EntityResponse::online`/`last_disconnected_at`, exposed here so
/// `live_ws`'s per-entity route can push live updates for those two fields
/// alongside the live-connections snapshot (today they're only loaded once,
/// via `GET /api/entities/{id}`).
pub async fn build_entity_status(
    state: &AppState,
    entity_id: Uuid,
) -> Result<(bool, Option<OffsetDateTime>), WebError> {
    ConnectionLog::entity_status(&state.db, entity_id)
        .await
        .map_err(WebError::Core)
}

// ── Admin response — one flat row per leg ─────────────────────────────────────

#[derive(Serialize)]
pub struct LiveConnectionRow {
    pub live: bool,
    pub remote_status: Option<RemoteStatus>,
    pub account: AccountRef,
    pub entity: EntityRef,
    pub role: &'static str,
    pub service_name: String,
    pub port: i32,
    pub peer_ip: Option<String>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub connected_since: Option<OffsetDateTime>,
}

pub async fn list_admin_live_connections(
    AdminUser(_admin): AdminUser,
    State(state): State<AppState>,
) -> Result<Json<Vec<LiveConnectionRow>>, WebError> {
    Ok(Json(build_admin_live_connections(&state).await?))
}

/// Reusable snapshot builder — shared by the HTTP route above and the
/// `admin/live-connections/ws` WebSocket route (`routes::live_ws`). Callers
/// are responsible for their own `AdminUser` check before calling this.
pub async fn build_admin_live_connections(state: &AppState) -> Result<Vec<LiveConnectionRow>, WebError> {
    let live_slots: HashSet<(Uuid, u32)> = state
        .server_slots
        .lock()
        .await
        .keys()
        .copied()
        .collect();
    let active_tunnels: Vec<tunnel2tunnel_ssh::ActiveTunnelInfo> =
        state.active_tunnels.lock().await.values().cloned().collect();

    let mut rows = Vec::new();

    // Server-role rows: every port_configs row across all entities.
    let server_pcs = PortConfig::list_all_with_owner(&state.db)
        .await
        .map_err(WebError::Core)?;
    let server_entity_ids: Vec<Uuid> = server_pcs.iter().map(|pc| pc.port_config.entity_id).collect();
    let server_entity_statuses = ConnectionLog::entity_statuses(&state.db, &server_entity_ids)
        .await
        .map_err(WebError::Core)?;
    for pc in server_pcs {
        let live = active_tunnels.iter().any(|info| {
            info.target_entity_id == pc.port_config.entity_id
                && info.proxy_port == pc.port_config.proxy_port as u32
        });
        let entity_online = server_entity_statuses
            .get(&pc.port_config.entity_id)
            .map(|(online, _)| *online)
            .unwrap_or(false);
        let port_forwarded =
            live_slots.contains(&(pc.port_config.entity_id, pc.port_config.proxy_port as u32));
        let remote_status = remote_status_for(pc.port_config.enabled, entity_online, port_forwarded, live);
        rows.push(LiveConnectionRow {
            live,
            remote_status: Some(remote_status),
            account: AccountRef {
                user_id: pc.owner_user_id,
                username: pc.owner_username,
            },
            entity: EntityRef {
                id: pc.port_config.entity_id,
                name: pc.owner_entity_name,
            },
            role: "server",
            service_name: pc.port_config.name,
            port: pc.port_config.proxy_port,
            peer_ip: None,
            connected_since: None,
        });
    }

    // Client-role rows: every port_subscriptions row across all entities.
    let subs = PortSubscription::list_all_with_context(&state.db)
        .await
        .map_err(WebError::Core)?;
    let owner_ids: Vec<Uuid> = subs.iter().map(|s| s.port_config.entity_id).collect();
    let owner_statuses = ConnectionLog::entity_statuses(&state.db, &owner_ids)
        .await
        .map_err(WebError::Core)?;
    for s in subs {
        let bridge = active_tunnels.iter().find(|info| {
            info.client_entity_id == s.subscription.subscriber_entity_id
                && info.port_config_id == s.port_config.id
        });
        let owner_online = owner_statuses
            .get(&s.port_config.entity_id)
            .map(|(online, _)| *online)
            .unwrap_or(false);
        let owner_port_live =
            live_slots.contains(&(s.port_config.entity_id, s.port_config.proxy_port as u32));
        let live = subscription_live(s.subscription.enabled, bridge.is_some());
        let remote_status =
            remote_status_for(s.subscription.enabled, owner_online, owner_port_live, bridge.is_some());
        rows.push(LiveConnectionRow {
            live,
            remote_status: Some(remote_status),
            account: AccountRef {
                user_id: s.subscriber_user_id,
                username: s.subscriber_username,
            },
            entity: EntityRef {
                id: s.subscription.subscriber_entity_id,
                name: s.subscriber_entity_name,
            },
            role: "client",
            service_name: s.port_config.name,
            port: s.subscription.subscriber_local_port,
            peer_ip: bridge.map(|b| b.peer_ip.clone()),
            connected_since: bridge.map(|b| b.since),
        });
    }

    Ok(rows)
}

// ── Aggregate builder — "my live connections" across all owned entities ───────

/// One flattened row (service or subscription leg) for the dashboard's
/// "your live connections" table — mirrors `LiveConnectionRow` but scoped to
/// a single user's own entities rather than every entity in the system, and
/// without the `account`/`role` breakdown admin needs (every row here is
/// already known to belong to `user_id`).
#[derive(Serialize)]
pub struct DashboardRow {
    pub live: bool,
    pub remote_status: Option<RemoteStatus>,
    pub entity_id: Uuid,
    pub entity_name: Option<String>,
    pub role: &'static str,
    pub service_name: String,
    pub port: i32,
    #[serde(with = "time::serde::rfc3339::option")]
    pub connected_since: Option<OffsetDateTime>,
}

/// Reusable snapshot builder for `GET /api/me/live-connections/ws`
/// (`routes::live_ws`) — replaces the frontend's previous N+1 client-side
/// fetch (one `build_entity_live_connections` HTTP call per owned entity)
/// with a single server-side loop over the same builder.
pub async fn build_my_live_connections(
    state: &AppState,
    user_id: Uuid,
) -> Result<Vec<DashboardRow>, WebError> {
    let entities = Entity::list_for_user(&state.db, user_id).await?;
    let mut rows = Vec::new();
    for entity in entities {
        let snapshot = build_entity_live_connections(state, entity.id).await?;
        for service in snapshot.services {
            rows.push(DashboardRow {
                live: service.live,
                remote_status: service.remote_status,
                entity_id: entity.id,
                entity_name: entity.name.clone(),
                role: "server",
                service_name: service.service_name,
                port: service.proxy_port,
                connected_since: service.subscribers.first().map(|s| s.connected_since),
            });
        }
        for sub in snapshot.subscriptions {
            rows.push(DashboardRow {
                live: sub.live,
                remote_status: sub.remote_status,
                entity_id: entity.id,
                entity_name: entity.name.clone(),
                role: "client",
                service_name: sub.service_name,
                port: sub.subscriber_local_port,
                connected_since: sub.connected_since,
            });
        }
    }
    Ok(rows)
}

// ── Shared status logic ────────────────────────────────────────────────────────

/// Dot for a subscriber-side row: is *my own* bridge live right now?
/// Disabled subscriptions are always dark (paused, not an error).
fn subscription_live(enabled: bool, has_active_bridge: bool) -> bool {
    enabled && has_active_bridge
}

/// Ring for either side of a row: shared 4-state logic, driven by whichever
/// entity is the "remote" one from this row's point of view — the owner
/// for a subscription row, or the entity itself for a service row.
///   - disabled row (subscription/port not enabled) -> offline (paused, not an error)
///   - remote not SSH-connected                      -> offline
///   - remote SSH-connected, port not forwarded       -> not_forwarded (the originally reported bug's exact case)
///   - remote SSH-connected, port forwarded, no bridge -> idle
///   - an active bridge exists right now              -> active (matches `live`)
fn remote_status_for(enabled: bool, online: bool, port_forwarded: bool, bridge_active: bool) -> RemoteStatus {
    if !enabled || !online {
        RemoteStatus::Offline
    } else if bridge_active {
        RemoteStatus::Active
    } else if port_forwarded {
        RemoteStatus::Idle
    } else {
        RemoteStatus::NotForwarded
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_requires_enabled_and_bridge() {
        assert!(!subscription_live(false, true));
        assert!(!subscription_live(false, false));
        assert!(!subscription_live(true, false));
        assert!(subscription_live(true, true));
    }

    #[test]
    fn remote_status_disabled_is_always_offline() {
        assert_eq!(remote_status_for(false, true, true, true), RemoteStatus::Offline);
        assert_eq!(remote_status_for(false, false, false, false), RemoteStatus::Offline);
    }

    #[test]
    fn remote_status_remote_offline_is_offline() {
        assert_eq!(remote_status_for(true, false, false, false), RemoteStatus::Offline);
        assert_eq!(remote_status_for(true, false, true, false), RemoteStatus::Offline);
    }

    #[test]
    fn remote_status_online_but_port_not_forwarded_is_not_forwarded() {
        assert_eq!(remote_status_for(true, true, false, false), RemoteStatus::NotForwarded);
    }

    #[test]
    fn remote_status_forwarded_but_no_bridge_is_idle() {
        // The originally reported bug's exact scenario: port forwarded, but
        // nobody is actively bridged through it right now.
        assert_eq!(remote_status_for(true, true, true, false), RemoteStatus::Idle);
    }

    #[test]
    fn remote_status_active_bridge_wins_over_forwarded_state() {
        assert_eq!(remote_status_for(true, true, true, true), RemoteStatus::Active);
        assert_eq!(remote_status_for(true, true, false, true), RemoteStatus::Active);
    }

    #[test]
    fn remote_status_enum_round_trips_snake_case() {
        assert_eq!(
            serde_json::to_string(&RemoteStatus::NotForwarded).unwrap(),
            "\"not_forwarded\""
        );
    }
}
