use std::collections::HashMap;

use serde::Serialize;
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::error::CoreError;
use crate::timestamps::Timestamps;

#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct ConnectionLog {
    pub id: Uuid,
    pub entity_id: Option<Uuid>,
    pub user_id: Option<Uuid>,
    pub peer_ip: Option<String>,
    pub key_fingerprint: Option<String>,
    pub attempted_password: Option<String>,
    pub attempted_username: Option<String>,
    pub fail_reason: Option<String>,
    pub success_reason: Option<String>,
    pub success: bool,
    pub tarpit_method: Option<String>,
    pub tarpit_action: Option<String>,
    pub tarpit_threshold_id: Option<Uuid>,
    pub banned_by_ban_rule_id: Option<Uuid>,
    pub ssh_flags: Option<String>,
    pub ports_requested: Option<String>,
    pub started_at: OffsetDateTime,
    pub ended_at: Option<OffsetDateTime>,
    #[sqlx(flatten)]
    pub ts: Timestamps,
}

impl ConnectionLog {
    #[allow(clippy::too_many_arguments)]
    pub async fn create(
        pool: &PgPool,
        entity_id: Option<Uuid>,
        user_id: Option<Uuid>,
        peer_ip: Option<&str>,
        key_fingerprint: Option<&str>,
        attempted_password: Option<&str>,
        attempted_username: Option<&str>,
        fail_reason: Option<&str>,
        success_reason: Option<&str>,
        tarpit_method: Option<&str>,
        tarpit_action: Option<&str>,
        tarpit_threshold_id: Option<Uuid>,
        banned_by_ban_rule_id: Option<Uuid>,
        started_at: OffsetDateTime,
    ) -> Result<Self, CoreError> {
        let id = Uuid::now_v7();
        sqlx::query_as::<_, ConnectionLog>(
            "INSERT INTO connection_logs \
               (id, entity_id, user_id, peer_ip, key_fingerprint, attempted_password, \
                attempted_username, fail_reason, success_reason, tarpit_method, \
                tarpit_action, tarpit_threshold_id, banned_by_ban_rule_id, started_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14) \
             RETURNING *",
        )
        .bind(id)
        .bind(entity_id)
        .bind(user_id)
        .bind(peer_ip)
        .bind(key_fingerprint)
        .bind(attempted_password)
        .bind(attempted_username)
        .bind(fail_reason)
        .bind(success_reason)
        .bind(tarpit_method)
        .bind(tarpit_action)
        .bind(tarpit_threshold_id)
        .bind(banned_by_ban_rule_id)
        .bind(started_at)
        .fetch_one(pool)
        .await
        .map_err(CoreError::Sqlx)
    }

    pub async fn set_ended(pool: &PgPool, id: Uuid) -> Result<(), CoreError> {
        sqlx::query("UPDATE connection_logs SET ended_at = NOW() WHERE id = $1")
            .bind(id)
            .execute(pool)
            .await
            .map_err(CoreError::Sqlx)?;
        Ok(())
    }

    pub async fn list_for_entity(
        pool: &PgPool,
        entity_id: Uuid,
        limit: i64,
    ) -> Result<Vec<Self>, CoreError> {
        sqlx::query_as::<_, ConnectionLog>(
            "SELECT * FROM connection_logs \
             WHERE entity_id = $1 ORDER BY started_at DESC LIMIT $2",
        )
        .bind(entity_id)
        .bind(limit)
        .fetch_all(pool)
        .await
        .map_err(CoreError::Sqlx)
    }

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
        .bind(peer_ip)
        .bind(since)
        .fetch_one(pool)
        .await
        .map_err(CoreError::Sqlx)
    }

    pub async fn count_recent_failures_for_user(
        pool: &PgPool,
        user_id: Uuid,
        since: OffsetDateTime,
    ) -> Result<i64, CoreError> {
        sqlx::query_scalar(
            "SELECT COUNT(*) FROM connection_logs \
             WHERE user_id = $1 AND fail_reason IS NOT NULL AND started_at >= $2",
        )
        .bind(user_id)
        .bind(since)
        .fetch_one(pool)
        .await
        .map_err(CoreError::Sqlx)
    }

    /// True if this peer_ip has ever completed a genuine successful login —
    /// used to permanently exclude it from the banner-drip tarpit method
    /// (which must intercept before russh ever runs, so it cannot tell a
    /// legitimate device apart from an attacker sharing the same IP by any
    /// other means).
    pub async fn peer_ip_has_known_good_history(
        pool: &PgPool,
        peer_ip: &str,
    ) -> Result<bool, CoreError> {
        sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM connection_logs \
             WHERE peer_ip = $1 AND success_reason = 'correct login')",
        )
        .bind(peer_ip)
        .fetch_one(pool)
        .await
        .map_err(CoreError::Sqlx)
    }

    /// Paginated/filtered/searched admin browser query. Returns (rows, total).
    #[allow(clippy::too_many_arguments)]
    pub async fn search(
        pool: &PgPool,
        peer_ip: Option<&str>,
        user_id: Option<Uuid>,
        success: Option<bool>,
        tarpit_method: Option<&str>,
        tarpit_method_present: Option<bool>,
        tarpit_action: Option<&str>,
        tarpit_action_present: Option<bool>,
        q: Option<&str>,
        started_at_gte: Option<OffsetDateTime>,
        started_at_lte: Option<OffsetDateTime>,
        ended_at_gte: Option<OffsetDateTime>,
        ended_at_lte: Option<OffsetDateTime>,
        page: i64,
        page_size: i64,
    ) -> Result<(Vec<Self>, i64), CoreError> {
        let offset = (page - 1).max(0) * page_size;
        let rows = sqlx::query_as::<_, ConnectionLog>(
            "SELECT * FROM connection_logs \
             WHERE ($1::text IS NULL OR peer_ip = $1) \
               AND ($2::uuid IS NULL OR user_id = $2) \
               AND ($3::bool IS NULL OR success = $3) \
               AND ($4::text IS NULL OR tarpit_method = $4) \
               AND ($5::bool IS NULL OR (tarpit_method IS NOT NULL) = $5) \
               AND ($6::text IS NULL OR tarpit_action = $6) \
               AND ($7::bool IS NULL OR (tarpit_action IS NOT NULL) = $7) \
               AND ($8::text IS NULL \
                    OR peer_ip ILIKE '%' || $8 || '%' \
                    OR key_fingerprint ILIKE '%' || $8 || '%' \
                    OR attempted_password ILIKE '%' || $8 || '%' \
                    OR fail_reason ILIKE '%' || $8 || '%') \
               AND ($9::timestamptz IS NULL OR started_at >= $9) \
               AND ($10::timestamptz IS NULL OR started_at <= $10) \
               AND ($11::timestamptz IS NULL OR ended_at >= $11) \
               AND ($12::timestamptz IS NULL OR ended_at <= $12) \
             ORDER BY started_at DESC \
             LIMIT $13 OFFSET $14",
        )
        .bind(peer_ip)
        .bind(user_id)
        .bind(success)
        .bind(tarpit_method)
        .bind(tarpit_method_present)
        .bind(tarpit_action)
        .bind(tarpit_action_present)
        .bind(q)
        .bind(started_at_gte)
        .bind(started_at_lte)
        .bind(ended_at_gte)
        .bind(ended_at_lte)
        .bind(page_size)
        .bind(offset)
        .fetch_all(pool)
        .await
        .map_err(CoreError::Sqlx)?;

        let total: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM connection_logs \
             WHERE ($1::text IS NULL OR peer_ip = $1) \
               AND ($2::uuid IS NULL OR user_id = $2) \
               AND ($3::bool IS NULL OR success = $3) \
               AND ($4::text IS NULL OR tarpit_method = $4) \
               AND ($5::bool IS NULL OR (tarpit_method IS NOT NULL) = $5) \
               AND ($6::text IS NULL OR tarpit_action = $6) \
               AND ($7::bool IS NULL OR (tarpit_action IS NOT NULL) = $7) \
               AND ($8::text IS NULL \
                    OR peer_ip ILIKE '%' || $8 || '%' \
                    OR key_fingerprint ILIKE '%' || $8 || '%' \
                    OR attempted_password ILIKE '%' || $8 || '%' \
                    OR fail_reason ILIKE '%' || $8 || '%') \
               AND ($9::timestamptz IS NULL OR started_at >= $9) \
               AND ($10::timestamptz IS NULL OR started_at <= $10) \
               AND ($11::timestamptz IS NULL OR ended_at >= $11) \
               AND ($12::timestamptz IS NULL OR ended_at <= $12)",
        )
        .bind(peer_ip)
        .bind(user_id)
        .bind(success)
        .bind(tarpit_method)
        .bind(tarpit_method_present)
        .bind(tarpit_action)
        .bind(tarpit_action_present)
        .bind(q)
        .bind(started_at_gte)
        .bind(started_at_lte)
        .bind(ended_at_gte)
        .bind(ended_at_lte)
        .fetch_one(pool)
        .await
        .map_err(CoreError::Sqlx)?;

        Ok((rows, total))
    }

    /// Online/offline + last-disconnect status for a single entity, derived
    /// purely from existing connection_logs rows (no dedicated column): an
    /// entity is online iff it has a successful session with no `ended_at` yet.
    pub async fn entity_status(
        pool: &PgPool,
        entity_id: Uuid,
    ) -> Result<(bool, Option<OffsetDateTime>), CoreError> {
        let row: Option<(bool, Option<OffsetDateTime>)> = sqlx::query_as(
            "SELECT COALESCE(bool_or(ended_at IS NULL), false), MAX(ended_at) \
             FROM connection_logs WHERE entity_id = $1 AND success",
        )
        .bind(entity_id)
        .fetch_optional(pool)
        .await
        .map_err(CoreError::Sqlx)?;
        Ok(row.unwrap_or((false, None)))
    }

    /// Batch variant of [`Self::entity_status`] to avoid N+1 queries on the
    /// entities list page.
    pub async fn entity_statuses(
        pool: &PgPool,
        entity_ids: &[Uuid],
    ) -> Result<HashMap<Uuid, (bool, Option<OffsetDateTime>)>, CoreError> {
        if entity_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let rows: Vec<(Uuid, bool, Option<OffsetDateTime>)> = sqlx::query_as(
            "SELECT entity_id, bool_or(ended_at IS NULL), MAX(ended_at) \
             FROM connection_logs \
             WHERE entity_id = ANY($1) AND success AND entity_id IS NOT NULL \
             GROUP BY entity_id",
        )
        .bind(entity_ids)
        .fetch_all(pool)
        .await
        .map_err(CoreError::Sqlx)?;

        Ok(rows
            .into_iter()
            .map(|(id, online, last_disconnected_at)| (id, (online, last_disconnected_at)))
            .collect())
    }
}
