# Tarpit/Ban Threshold Engine — Full Investigation Report

## 1. `crates/tunnel2tunnel-ssh/src/tarpit/mod.rs` — full detail

File is 443 lines total. Key pieces:

**Module doc (lines 1-6):**
```rust
//! fail2ban-style ban rules + round-robined SSH tarpit methods.
//!
//! All new tarpit logic lives under this module (never inline in `lib.rs`),
//! per project convention. Sibling modules implement the three tarpit
//! methods; this module owns the shared in-memory ban/threshold state and
//! the decision logic for which method (if any) applies to a connection.
```

**`TarpitEntry` struct (lines 69-96):**
```rust
#[derive(Debug)]
pub(crate) struct TarpitEntry {
    fail_count: u32,
    window_start: Instant,
    trigger_count: u64,
    banner_drip_eligible: bool,
    banned: BanUntil,
}
```
`is_banned()` just delegates to `self.banned.is_active(now)`.

**`TarpitState` type (lines 98-103):**
```rust
/// Key: peer_ip directly, or `user:{uuid}` for user-scoped state.
pub type TarpitState = Arc<Mutex<HashMap<String, TarpitEntry>>>;

pub fn new_state() -> TarpitState {
    Arc::new(Mutex::new(HashMap::new()))
}
```
It's a single flat map keyed by either the raw `peer_ip` string or `format!("user:{user_id}")` (from `user_key()`, line 105-107). There is exactly **one** `TarpitEntry` per key — i.e. one counter/window per identity, not per-rule.

**`Thresholds` (the current single count+window+enabled struct, lines 109-126):**
```rust
#[derive(Debug, Clone)]
pub struct Thresholds {
    pub count: u32,
    pub window: Duration,
    pub enabled: bool,
}

impl Default for Thresholds {
    fn default() -> Self {
        Self { count: 5, window: Duration::from_secs(600), enabled: true }
    }
}

pub type SharedThresholds = Arc<Mutex<Thresholds>>;
```
This `Thresholds` struct is exactly what would need to become `Vec<ThresholdRule>` (or similar) for the multi-rule redesign.

**`record_failure()` — the actual threshold-check/ban-trigger function (lines 130-159):**
```rust
fn record_failure(
    map: &mut HashMap<String, TarpitEntry>,
    key: &str,
    thresholds: &Thresholds,
    now: Instant,
) -> bool {
    let entry = map
        .entry(key.to_string())
        .or_insert_with(|| TarpitEntry::new(now));

    if now.duration_since(entry.window_start) > thresholds.window {
        entry.fail_count = 0;
        entry.window_start = now;
    }

    entry.fail_count += 1;

    if entry.fail_count >= thresholds.count {
        entry.trigger_count += 1;
        entry.banned = BanUntil::At(now + thresholds.window);
        entry.fail_count = 0;
        entry.window_start = now;
    }

    entry.is_banned(now)
}
```
This is a single-window sliding-bucket-reset algorithm (not a true sliding window — it resets the whole tally when `window_start` gets stale, rather than counting failures in a rolling window). It's the sole place that "checks threshold_count/threshold_window_seconds and decides to trigger a ban." For "5 in 10min AND 20 in 24h AND 50 in a week" you'd need either three independent `TarpitEntry`-like counters per identity (one per rule, each with its own `fail_count`/`window_start`/`window` duration) evaluated independently, or a redesign to real timestamped-event lists per identity so multiple windows can be evaluated against the same event history.

- `trigger_count` (line 73, incremented at line 152) is *only* used to drive `TarpitMethod::round_robin(trigger_count)` (lines 40-49) — it picks which of the 3 tarpit methods (BannerDrip/SlowAuth/FakeShell) to use, cycling `trigger_count % 3`. It has no other semantic meaning (not exposed to settings/admin).
- `fail_count` is the per-window running tally reset either when stale (window elapsed) or immediately after a ban is triggered.

**`clear_fail_tally_on_success()` (lines 167-172)** — success resets `fail_count`/`window_start` but never clears an existing `banned` state (comment lines 161-166 explains rationale).

**Async wrappers (lines 176-203):** `record_auth_failure()` calls `record_failure()` for both peer_ip and (if present) `user_key(uid)`; short-circuits entirely if `!thresholds.enabled` (line 182). `record_auth_success()` calls `clear_fail_tally_on_success()` similarly.

**Settings refresh loop — where `Settings::get(pool, "tarpit_threshold_count"/"tarpit_threshold_window_seconds"/"tarpit_enabled")` are read (lines 279-344):**
```rust
pub fn spawn_settings_refresher(pool: PgPool, state: TarpitState, thresholds: SharedThresholds) {
    tokio::spawn(async move {
        loop {
            refresh_once(&pool, &state, &thresholds).await;
            tokio::time::sleep(Duration::from_secs(30)).await;
        }
    });
}

async fn refresh_once(pool: &PgPool, state: &TarpitState, thresholds: &SharedThresholds) {
    let count = Settings::get(pool, "tarpit_threshold_count")
        .await.ok().flatten().and_then(|v| v.parse::<u32>().ok()).unwrap_or(5);
    let window_secs = Settings::get(pool, "tarpit_threshold_window_seconds")
        .await.ok().flatten().and_then(|v| v.parse::<u64>().ok()).unwrap_or(600);
    let enabled = Settings::get(pool, "tarpit_enabled")
        .await.ok().flatten().map(|v| v == "true").unwrap_or(true);

    {
        let mut t = thresholds.lock().await;
        t.count = count;
        t.window = Duration::from_secs(window_secs);
        t.enabled = enabled;
    }

    let Ok(rules) = BanRule::list_active(pool).await else { return; };
    // ... merges ban_rules into the in-memory map (lines 322-343)
}
```
This is the *only* place the three settings keys are read on the SSH-server side; it polls every 30s and writes into `SharedThresholds` (an `Arc<Mutex<Thresholds>>`), which is what `record_auth_failure` actually consults per-attempt (not a direct per-call `Settings::get`). `refresh_once` also merges `BanRule::list_active()` rows into the same in-memory map (admin-created indefinite/expiring bans), separate mechanism from the threshold auto-ban.

Every occurrence of the three setting keys across the repo (grep results):
- `crates/tunnel2tunnel-ssh/src/tarpit/mod.rs:292,298,304` (reads, above)
- `crates/tunnel2tunnel-web/src/routes/tarpit.rs:195,200,205` (admin GET reads)
- `crates/tunnel2tunnel-web/src/routes/tarpit.rs:229,234,239` (admin PUT writes)

No other files reference these keys (frontend uses the JSON API, not the keys directly).

## 2. `count_recent_failures_for_peer_ip` / `count_recent_failures_for_user` call sites

Defined in `/home/user/git/luckydonald/tunnel2tunnel/crates/tunnel2tunnel-core/src/models/connection_log.rs:97-127`:
```rust
/// Failed-attempt count for a peer_ip within the given window, used by the
/// tarpit/ban threshold engine. Only counts rows that actually have a
/// fail_reason (i.e. a real credential rejection) — `auth_none` probes and
/// infra/db errors never write a row at all, so they never show up here.
pub async fn count_recent_failures_for_peer_ip(
    pool: &PgPool,
    peer_ip: &str,
    since: OffsetDateTime,
) -> Result<i64, CoreError> {
    sqlx::query_scalar(
        "SELECT COUNT(*) FROM connection_logs \
         WHERE peer_ip = $1 AND fail_reason IS NOT NULL AND started_at >= $2",
    )
    .bind(peer_ip).bind(since).fetch_one(pool).await.map_err(CoreError::Sqlx)
}

pub async fn count_recent_failures_for_user(
    pool: &PgPool,
    user_id: Uuid,
    since: OffsetDateTime,
) -> Result<i64, CoreError> { ... }
```

**Grep for all call sites across `crates/`:**
```
crates/t2t/tests/tarpit_e2e.rs:202:    let count = ConnectionLog::count_recent_failures_for_peer_ip(&pool, "127.0.0.1", since)
crates/t2t/tests/tarpit_e2e.rs:204:        .expect("count_recent_failures_for_peer_ip");
crates/tunnel2tunnel-core/src/models/connection_log.rs:97:    pub async fn count_recent_failures_for_peer_ip(
crates/tunnel2tunnel-core/src/models/connection_log.rs:113:    pub async fn count_recent_failures_for_user(
crates/tunnel2tunnel-core/tests/tarpit_models.rs:117:    let count = ConnectionLog::count_recent_failures_for_peer_ip(&pool, &peer_ip, since)
```

**Conclusion: these two functions are called ONLY from test files** (`crates/t2t/tests/tarpit_e2e.rs` and `crates/tunnel2tunnel-core/tests/tarpit_models.rs`), never from production code (`tarpit/mod.rs`, `tunnel2tunnel-web/src/routes/tarpit.rs`, or the ssh lib). The actual ban-trigger decision at connection time is **purely in-memory** via `TarpitEntry`/`record_failure()` — it does not query `connection_logs` at all per attempt. This is an important architectural fact for the redesign: if the new multi-rule engine wants to evaluate windows like "20 fails in 24h" or "50 in a week" that outlive process restarts/exceed practical in-memory retention, it currently has no DB-backed path wired in — the DB counters exist but are dead code in production, only exercised by tests as of now.

## 3. Admin frontend — tarpit settings form

There is no `Settings.vue`; the tarpit settings form lives in **`/home/user/git/luckydonald/tunnel2tunnel/frontend/src/pages/AdminBanRulesPage.vue`** (245 lines), combined with ban-rule CRUD on the same page.

**API client (`/home/user/git/luckydonald/tunnel2tunnel/frontend/src/api/admin.ts:65-69, 178-184`):**
```ts
export interface TarpitSettings {
  threshold_count: number
  threshold_window_seconds: number
  enabled: boolean
}
...
  getTarpitSettings: () => apiFetch<TarpitSettings>('/api/admin/tarpit-settings'),

  updateTarpitSettings: (settings: TarpitSettings) =>
    apiFetch<void>('/api/admin/tarpit-settings', {
      method: 'PUT',
      body: JSON.stringify(settings),
    }),
```

**Script section (`AdminBanRulesPage.vue:70-98`):**
```vue
// Global thresholds
const settings = ref<TarpitSettings | null>(null)
const savingSettings = ref(false)

async function loadSettings(): Promise<void> {
  try {
    settings.value = await adminApi.getTarpitSettings()
  } catch (e) {
    toast(e instanceof Error ? e.message : 'Failed to load tarpit settings')
  }
}

async function saveSettings(): Promise<void> {
  if (!settings.value) return
  savingSettings.value = true
  try {
    await adminApi.updateTarpitSettings(settings.value)
    toast('Settings saved')
  } catch (e) {
    toast(e instanceof Error ? e.message : 'Failed to save settings')
  } finally {
    savingSettings.value = false
  }
}

onMounted(() => {
  loadRules()
  loadSettings()
})
```

**Template section (`AdminBanRulesPage.vue:107-125`):**
```vue
<section class="section">
  <h2>Global Thresholds</h2>
  <div v-if="settings" class="thresholds-card">
    <label class="checkbox-label">
      <input v-model="settings.enabled" type="checkbox" /> Tarpit/ban enforcement enabled
    </label>
    <div class="field-inline">
      <label>Failed attempts before ban</label>
      <input v-model.number="settings.threshold_count" type="number" min="1" class="input-sm" />
    </div>
    <div class="field-inline">
      <label>Window (seconds)</label>
      <input v-model.number="settings.threshold_window_seconds" type="number" min="1" class="input-sm" />
    </div>
    <button class="btn-primary" :disabled="savingSettings" @click="saveSettings">
      {{ savingSettings ? 'Saving…' : 'Save' }}
    </button>
  </div>
</section>
```
This single `.thresholds-card` block (one checkbox + two number inputs + one save button) is exactly what needs to become a repeatable rows-table UI (à la the "Rules" section further down the same page, which already has an add-form + data-table pattern at lines 127-175 that would be a natural template to copy for multi-threshold rows).

Backend route handlers backing this page: `/home/user/git/luckydonald/tunnel2tunnel/crates/tunnel2tunnel-web/src/routes/tarpit.rs:177-243` (`TarpitSettingsResponse`, `UpdateTarpitSettingsBody`, `get_tarpit_settings`, `update_tarpit_settings` — includes validation at lines 223-228 rejecting zero count/window).

## 4. `BanRule` model — full content (for CRUD template reuse)

`/home/user/git/luckydonald/tunnel2tunnel/crates/tunnel2tunnel-core/src/models/ban_rule.rs` (81 lines, quoted in full):
```rust
use serde::Serialize;
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::error::CoreError;
use crate::timestamps::Timestamps;

#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct BanRule {
    pub id: Uuid,
    pub scope_type: String,
    pub peer_ip: Option<String>,
    pub user_id: Option<Uuid>,
    pub reason: Option<String>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub active_until: Option<OffsetDateTime>,
    pub created_by: Uuid,
    #[sqlx(flatten)]
    #[serde(flatten)]
    pub ts: Timestamps,
}

impl BanRule {
    /// Rules currently in effect (no expiry, or expiry still in the future).
    pub async fn list_active(pool: &PgPool) -> Result<Vec<Self>, CoreError> {
        sqlx::query_as::<_, BanRule>(
            "SELECT * FROM ban_rules \
             WHERE active_until IS NULL OR active_until > NOW() \
             ORDER BY created_at DESC",
        )
        .fetch_all(pool)
        .await
        .map_err(CoreError::Sqlx)
    }

    /// All rules, including expired ones — used by the admin CRUD listing.
    pub async fn list_all(pool: &PgPool) -> Result<Vec<Self>, CoreError> {
        sqlx::query_as::<_, BanRule>("SELECT * FROM ban_rules ORDER BY created_at DESC")
            .fetch_all(pool)
            .await
            .map_err(CoreError::Sqlx)
    }

    pub async fn create(
        pool: &PgPool,
        scope_type: &str,
        peer_ip: Option<&str>,
        user_id: Option<Uuid>,
        reason: Option<&str>,
        active_until: Option<OffsetDateTime>,
        created_by: Uuid,
    ) -> Result<Self, CoreError> {
        let id = Uuid::now_v7();
        sqlx::query_as::<_, BanRule>(
            "INSERT INTO ban_rules \
               (id, scope_type, peer_ip, user_id, reason, active_until, created_by) \
             VALUES ($1, $2, $3, $4, $5, $6, $7) \
             RETURNING *",
        )
        .bind(id)
        .bind(scope_type)
        .bind(peer_ip)
        .bind(user_id)
        .bind(reason)
        .bind(active_until)
        .bind(created_by)
        .fetch_one(pool)
        .await
        .map_err(CoreError::Sqlx)
    }

    pub async fn delete(pool: &PgPool, id: Uuid) -> Result<bool, CoreError> {
        let r = sqlx::query("DELETE FROM ban_rules WHERE id = $1")
            .bind(id)
            .execute(pool)
            .await
            .map_err(CoreError::Sqlx)?;
        Ok(r.rows_affected() > 0)
    }
}
```
Pattern to replicate: derive `sqlx::FromRow + Serialize`, `id: Uuid` generated via `Uuid::now_v7()` in Rust (not DB default, unlike the migration's `DEFAULT uuidv7()` — note the inconsistency: schema has a DB default but `create()` still explicitly generates and binds an id), `list_active`/`list_all` split, single `create()` with all fields as params, boolean-returning `delete()`. `Timestamps` is a shared flattened sub-struct (`crates/tunnel2tunnel-core/src/timestamps.rs`, not fully read here but referenced via `#[sqlx(flatten)] #[serde(flatten)] pub ts: Timestamps`) that a new `tarpit_threshold` model could reuse identically (it already has `created_at`/`updated_at` semantics matching the `timestamps` trigger pattern used in migration 008 lines 83-84: `CREATE TRIGGER timestamps BEFORE INSERT OR UPDATE ON ban_rules FOR EACH ROW EXECUTE FUNCTION set_timestamps();`).

## 5. Migrations directory — numbering convention

```
migrations/000_helpers.sql
migrations/001_users.sql
migrations/002_entities_keys_ports.sql
migrations/003_access_friends.sql
migrations/004_connection_logs.sql
migrations/005_port_discovery.sql
migrations/006_entity_port_server_ref.sql
migrations/007_entity_port_host.sql
migrations/008_tarpit.sql
```
Confirmed: **`008_tarpit.sql` is the latest migration** (nothing at 009+). Convention is zero-padded 3-digit prefix + snake_case descriptive name (`NNN_description.sql`), so the new migration for a `tarpit_threshold`-rules table should be **`009_<descriptive_name>.sql`** (e.g. `009_tarpit_thresholds.sql`), following the pattern already used in `008_tarpit.sql`: a `CREATE TABLE` with `id UUID PRIMARY KEY DEFAULT uuidv7()`, a `CHECK` constraint for enum-like columns, `created_at`/`updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()`, and a `CREATE TRIGGER timestamps BEFORE INSERT OR UPDATE ... EXECUTE FUNCTION set_timestamps();` plus targeted partial indexes (see `008_tarpit.sql:67-88` for the `ban_rules` table exactly).

## Summary of redesign-relevant architecture facts

1. **Single global threshold today**: `Thresholds { count, window, enabled }` (one struct instance, `tarpit/mod.rs:109-126`), refreshed from 3 `settings` kv rows every 30s (`tarpit/mod.rs:291-317`), consumed by `record_failure()` (`tarpit/mod.rs:134-159`) which does simple reset-on-stale-window counting per identity key — **not** a true multi-window rolling counter.
2. **Purely in-memory decision**: no DB query happens per auth attempt for threshold checking; `connection_logs`-based counting functions exist but are dead code outside tests.
3. **Separate but adjacent mechanism**: `ban_rules` table (admin-created explicit bans by peer_ip/user, with optional expiry) is merged into the *same* in-memory map every 30s refresh cycle (`refresh_once`, lines 318-343) — auto-triggered threshold bans and manual `ban_rules` share the same `TarpitEntry.banned: BanUntil` field but are set by different code paths.
4. **Admin UI**: one page (`AdminBanRulesPage.vue`) handles both the ban_rules CRUD and the single-row threshold settings form; the ban_rules "add-form"/`data-table` (lines 127-175) is a ready-made multi-row UI pattern to copy for a new thresholds table.
5. **BanRule model** is the cleanest existing template for a new `TarpitThreshold` model — same crate (`tunnel2tunnel-core/src/models/`), same `Timestamps` flatten pattern, same `list_all`/`create`/`delete` CRUD shape.
6. **Next migration number is `009`.**