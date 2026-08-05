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
//!
//! All three former routes (per-entity, per-user "me", admin) are unified
//! into a single [`EntityLiveSnapshot`] shape and a single
//! [`build_live_connections`] query, parameterized by [`Scope`] — see
//! `routes::live_ws` for the WebSocket route that serves it and switches
//! scope on request.

use std::collections::HashSet;

use serde::Serialize;
use time::OffsetDateTime;
use tunnel2tunnel_core::models::{
    connection_log::ConnectionLog, entity::Entity, port_config::PortConfig,
    port_subscription::PortSubscription, user::User,
};
use uuid::Uuid;

use crate::{AppState, WebError};

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
    /// The subscriber's own bridge peer address — `None` when there's no
    /// active bridge right now. Always this viewer's own connection, so
    /// exposing it isn't scope-gated.
    pub peer_ip: Option<String>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub connected_since: Option<OffsetDateTime>,
}

#[derive(Serialize)]
pub struct EntityLiveConnectionsSnapshot {
    /// This entity's own `port_configs` ("my services"), each with its live
    /// status and currently-bridged subscribers.
    pub services: Vec<ServiceLiveStatus>,
    /// This entity's own `port_subscriptions` ("my subscriptions"), each
    /// with its own live status.
    pub subscriptions: Vec<SubscriptionLiveStatus>,
}

/// Reusable snapshot builder — called once per entity by
/// [`build_live_connections`]. Callers are responsible for their own auth
/// check before calling this (either "does the viewer own this entity" for
/// `Scope::Mine`, or admin-only for `Scope::All`).
pub async fn build_entity_live_connections(
    state: &AppState,
    entity_id: Uuid,
) -> Result<EntityLiveConnectionsSnapshot, WebError> {
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
            peer_ip: bridge.map(|b| b.peer_ip.clone()),
            connected_since: bridge.map(|b| b.since),
        });
    }

    Ok(EntityLiveConnectionsSnapshot {
        services,
        subscriptions,
    })
}

// ── Unified snapshot — one route, one shape, scoped by viewer ─────────────────

/// Which entities [`build_live_connections`] should include.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Scope {
    /// Only entities owned by the requesting user — the default for every
    /// connection.
    Mine,
    /// Every entity system-wide — admin-only, requested via the `set_scope`
    /// command on the WebSocket connection.
    All,
}

#[derive(Serialize)]
pub struct EntityLiveSnapshot {
    pub entity_id: Uuid,
    pub entity_name: Option<String>,
    pub entity_online: bool,
    #[serde(with = "time::serde::rfc3339::option")]
    pub entity_last_disconnected_at: Option<OffsetDateTime>,
    /// Owned by the viewer making this request — always `true` in
    /// `Scope::Mine`, computed per-row in `Scope::All` so the frontend (and
    /// specifically its toast logic) can tell the difference regardless of
    /// which scope the connection currently has.
    pub mine: bool,
    pub services: Vec<ServiceLiveStatus>,
    pub subscriptions: Vec<SubscriptionLiveStatus>,
    /// Admin-only: which user owns this entity. `None` unless the query ran
    /// in `Scope::All` — never sent to a non-admin viewer regardless, since
    /// only admins can ever request that scope.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account: Option<AccountRef>,
}

/// The one query backing `/api/live-connections/ws` in every scope. Still
/// calls [`build_entity_live_connections`] once per entity exactly as the
/// former per-user/admin builders did — this consolidation is "don't discard
/// the nested detail before serializing" plus "broaden the entity list in
/// `Scope::All`," not new query logic.
pub async fn build_live_connections(
    state: &AppState,
    user_id: Uuid,
    scope: Scope,
) -> Result<Vec<EntityLiveSnapshot>, WebError> {
    let entities = match scope {
        Scope::Mine => Entity::list_for_user(&state.db, user_id).await?,
        Scope::All => Entity::list_all(&state.db).await?,
    };

    let owner_usernames: std::collections::HashMap<Uuid, String> = if scope == Scope::All {
        User::list_all(&state.db)
            .await
            .map_err(WebError::Core)?
            .into_iter()
            .map(|u| (u.id, u.username))
            .collect()
    } else {
        std::collections::HashMap::new()
    };

    let mut snapshots = Vec::with_capacity(entities.len());
    for entity in entities {
        let snapshot = build_entity_live_connections(state, entity.id).await?;
        let (entity_online, entity_last_disconnected_at) =
            build_entity_status(state, entity.id).await?;
        let mine = entity.user_id == user_id;
        let account = if scope == Scope::All {
            owner_usernames.get(&entity.user_id).map(|username| AccountRef {
                user_id: entity.user_id,
                username: username.clone(),
            })
        } else {
            None
        };
        snapshots.push(EntityLiveSnapshot {
            entity_id: entity.id,
            entity_name: entity.name,
            entity_online,
            entity_last_disconnected_at,
            mine,
            services: snapshot.services,
            subscriptions: snapshot.subscriptions,
            account,
        });
    }
    Ok(snapshots)
}

/// Reusable single-entity online-status lookup — same source
/// (`ConnectionLog::entity_status`) that `routes::entities::get_entity` uses
/// for `EntityResponse::online`/`last_disconnected_at`.
pub async fn build_entity_status(
    state: &AppState,
    entity_id: Uuid,
) -> Result<(bool, Option<OffsetDateTime>), WebError> {
    ConnectionLog::entity_status(&state.db, entity_id)
        .await
        .map_err(WebError::Core)
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
