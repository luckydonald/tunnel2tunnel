//! Live connections dashboard — read-only views over the in-memory
//! `ServerSlots`/`ActiveTunnels` maps shared with the SSH server (see
//! `AppState` in `crates/tunnel2tunnel-web/src/lib.rs`), joined with the
//! `port_configs`/`port_subscriptions` tables for display context.
//!
//! Status-dot semantics (matches the plan's mockups):
//!   - green  = a live entry exists for that `(entity, port)` right now.
//!   - gray   = configured/subscribed, but nothing live currently.
//!   - orange = subscriber-side only: the subscription is enabled but the
//!              owner's port has no live `server_slots` entry at all (i.e.
//!              "the other side is down", not just "haven't connected yet").

use std::collections::HashSet;

use axum::{
    extract::{Path, State},
    Json,
};
use serde::Serialize;
use time::OffsetDateTime;
use tunnel2tunnel_core::models::{
    entity::Entity, port_config::PortConfig, port_subscription::PortSubscription, user::User,
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

#[derive(Serialize)]
pub struct ServiceLiveStatus {
    pub port_config_id: Uuid,
    pub service_name: String,
    pub proxy_port: i32,
    /// "green" | "gray" — a service you own is never "orange" (orange is a
    /// subscriber-side-only status).
    pub status: &'static str,
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
    pub status: &'static str,
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

    // ── My services ────────────────────────────────────────────────────────
    let owned_ports = PortConfig::list_for_entity(&state.db, entity_id).await?;
    let mut services = Vec::with_capacity(owned_ports.len());
    for pc in owned_ports {
        let live = live_slots.contains(&(entity_id, pc.proxy_port as u32));
        let mut subscribers = Vec::new();
        for info in active_tunnels
            .iter()
            .filter(|info| info.target_entity_id == entity_id && info.proxy_port == pc.proxy_port as u32)
        {
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
        services.push(ServiceLiveStatus {
            port_config_id: pc.id,
            service_name: pc.name,
            proxy_port: pc.proxy_port,
            status: if live { "green" } else { "gray" },
            subscribers,
        });
    }

    // ── My subscriptions ───────────────────────────────────────────────────
    let subs = PortSubscription::list_for_subscriber_with_context(&state.db, entity_id).await?;
    let mut subscriptions = Vec::with_capacity(subs.len());
    for s in subs {
        let bridge = active_tunnels.iter().find(|info| {
            info.client_entity_id == entity_id && info.port_config_id == s.port_config.id
        });
        let owner_live =
            live_slots.contains(&(s.port_config.entity_id, s.port_config.proxy_port as u32));
        let status = status_for_subscription(s.subscription.enabled, bridge.is_some(), owner_live);
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
            status,
            connected_since: bridge.map(|b| b.since),
        });
    }

    Ok(Json(EntityLiveConnectionsResponse {
        services,
        subscriptions,
    }))
}

// ── Admin response — one flat row per leg ─────────────────────────────────────

#[derive(Serialize)]
pub struct LiveConnectionRow {
    pub status: &'static str,
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
    for pc in server_pcs {
        let live = live_slots.contains(&(pc.port_config.entity_id, pc.port_config.proxy_port as u32));
        rows.push(LiveConnectionRow {
            status: if live { "green" } else { "gray" },
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
    for s in subs {
        let bridge = active_tunnels.iter().find(|info| {
            info.client_entity_id == s.subscription.subscriber_entity_id
                && info.port_config_id == s.port_config.id
        });
        let owner_live =
            live_slots.contains(&(s.port_config.entity_id, s.port_config.proxy_port as u32));
        let status = status_for_subscription(s.subscription.enabled, bridge.is_some(), owner_live);
        rows.push(LiveConnectionRow {
            status,
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

    Ok(Json(rows))
}

// ── Shared status logic ────────────────────────────────────────────────────────

/// Status-dot for a subscriber-side row/entry:
///   - disabled subscription           -> gray  (paused, not an error)
///   - enabled + live bridge           -> green
///   - enabled + owner port is live    -> gray  (configured, just not connected right now)
///   - enabled + owner port not live   -> orange (the other side is down)
fn status_for_subscription(enabled: bool, has_active_bridge: bool, owner_live: bool) -> &'static str {
    if !enabled {
        "gray"
    } else if has_active_bridge {
        "green"
    } else if owner_live {
        "gray"
    } else {
        "orange"
    }
}
