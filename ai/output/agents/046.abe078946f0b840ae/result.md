## Rust struct definitions (crates/tunnel2tunnel-web/src/routes/live_connections.rs)

**`RemoteStatus` enum** (lines 67-73):
```rust
#[derive(Serialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum RemoteStatus {
    Gray,
    Orange,
    Green,
}
```

**`ServiceLiveStatus`** (lines 75-85):
```rust
#[derive(Serialize)]
pub struct ServiceLiveStatus {
    pub port_config_id: Uuid,
    pub service_name: String,
    pub proxy_port: i32,
    /// Aggregated over current subscribers — see module doc comment.
    pub live: bool,
    /// Always `None` — a service has 0..N subscribers, no single ring target.
    pub remote_status: Option<RemoteStatus>,
    pub subscribers: Vec<SubscriberInfo>,
}
```
Fields: `port_config_id: Uuid` (required), `service_name: String` (required), `proxy_port: i32` (required), `live: bool` (required), `remote_status: Option<RemoteStatus>` (optional — comment says always `None`), `subscribers: Vec<SubscriberInfo>` (required, may be empty).

Confirmation: yes, `ServiceLiveStatus` **does** have a `remote_status: Option<RemoteStatus>` field. It is not just `live` + `subscribers` — it currently exists but is hard-coded to always be `None`.

**`SubscriptionLiveStatus`** (lines 87-100):
```rust
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
```
Fields: `subscription_id: Uuid`, `port_config_id: Uuid`, `owner: EntityRef`, `service_name: String`, `proxy_port: i32`, `subscriber_local_port: i32`, `enabled: bool`, `live: bool` — all required; `remote_status: Option<RemoteStatus>` (optional, set to `Some(...)`), `connected_since: Option<OffsetDateTime>` (optional).

## Construction code

**`services[]` loop** (lines 144-179):
```rust
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
    services.push(ServiceLiveStatus {
        port_config_id: pc.id,
        service_name: pc.name,
        proxy_port: pc.proxy_port,
        live,
        remote_status: None,
        subscribers,
    });
}
```

**`subscriptions[]` loop** (lines 187-215):
```rust
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
    let remote_status = subscription_remote_status(s.subscription.enabled, owner_online, owner_port_live);
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
```

## TypeScript types (frontend/src/api/entities.ts)

**`ServiceLiveStatus`** (lines 89-98):
```ts
export interface ServiceLiveStatus {
  port_config_id: string
  service_name: string
  proxy_port: number
  /** Aggregated over current subscribers — see `live_connections.rs`'s module doc comment. */
  live: boolean
  /** Always `null` — a service has 0..N subscribers, no single ring target. */
  remote_status: RemoteStatus | null
  subscribers: LiveSubscriberInfo[]
}
```
Fields: `port_config_id: string`, `service_name: string`, `proxy_port: number`, `live: boolean`, `remote_status: RemoteStatus | null`, `subscribers: LiveSubscriberInfo[]`.

Confirmation: yes — `remote_status: RemoteStatus | null` **does** appear on the service-side TS type, mirroring the Rust struct, with a comment stating it's always `null`.

**`SubscriptionLiveStatus`** (lines 100-111):
```ts
export interface SubscriptionLiveStatus {
  subscription_id: string
  port_config_id: string
  owner: LiveEntityRef
  service_name: string
  proxy_port: number
  subscriber_local_port: number
  enabled: boolean
  live: boolean
  remote_status: RemoteStatus | null
  connected_since: string | null
}
```
Fields: `subscription_id: string`, `port_config_id: string`, `owner: LiveEntityRef`, `service_name: string`, `proxy_port: number`, `subscriber_local_port: number`, `enabled: boolean`, `live: boolean`, `remote_status: RemoteStatus | null`, `connected_since: string | null`.