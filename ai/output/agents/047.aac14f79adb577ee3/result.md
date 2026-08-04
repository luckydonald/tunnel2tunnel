**PortConfig struct fields** (from `/home/user/git/luckydonald/tunnel2tunnel/crates/tunnel2tunnel-core/src/models/port_config.rs`, lines 14-29):

```rust
pub struct PortConfig {
    pub id: Uuid,
    pub entity_id: Uuid,
    pub enabled: bool,
    pub local_port: i32,
    pub proxy_port: i32,
    pub name: String,
    pub description: Option<String>,
    pub sort_order: i32,
    pub host: String,
    pub ts: Timestamps,
}
```

**`list_for_entity` signature:**

```rust
pub async fn list_for_entity(pool: &PgPool, entity_id: Uuid) -> Result<Vec<Self>, CoreError>
```