//! Admin-only routes for the SSH tarpit/ban-rules feature: paginated/filtered
//! connection-log browsing, ban-rule CRUD, and global threshold settings.
//! Kept in its own module, separate from the general entity/settings routes.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

use tunnel2tunnel_core::models::{
    ban_rule::BanRule, connection_log::ConnectionLog, settings::Settings,
    tarpit_threshold::TarpitThreshold,
};

use crate::{extractors::AdminUser, error::WebError, routes::entities::ConnLogResponse, AppState};

// ── Connection log search ───────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct LogSearchQuery {
    pub page: Option<i64>,
    pub page_size: Option<i64>,
    pub peer_ip: Option<String>,
    pub user_id: Option<Uuid>,
    pub success: Option<bool>,
    pub method: Option<String>,
    pub method_present: Option<bool>,
    pub action: Option<String>,
    pub action_present: Option<bool>,
    pub q: Option<String>,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub started_at_gte: Option<OffsetDateTime>,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub started_at_lte: Option<OffsetDateTime>,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub ended_at_gte: Option<OffsetDateTime>,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub ended_at_lte: Option<OffsetDateTime>,
}

#[derive(Serialize)]
pub struct LogSearchResponse {
    pub items: Vec<ConnLogResponse>,
    pub total: i64,
    pub page: i64,
    pub page_size: i64,
}

/// Pure pagination normalization — extracted so it's unit-testable without a
/// DB/HTTP request. Page is floored at 1; page_size clamped to [1, 200].
fn normalize_pagination(page: Option<i64>, page_size: Option<i64>) -> (i64, i64) {
    (page.unwrap_or(1).max(1), page_size.unwrap_or(50).clamp(1, 200))
}

pub async fn search_connection_logs(
    AdminUser(_admin): AdminUser,
    State(state): State<AppState>,
    Query(q): Query<LogSearchQuery>,
) -> Result<Json<LogSearchResponse>, WebError> {
    let (page, page_size) = normalize_pagination(q.page, q.page_size);
    let (rows, total) = ConnectionLog::search(
        &state.db,
        q.peer_ip.as_deref(),
        q.user_id,
        q.success,
        q.method.as_deref(),
        q.method_present,
        q.action.as_deref(),
        q.action_present,
        q.q.as_deref(),
        q.started_at_gte,
        q.started_at_lte,
        q.ended_at_gte,
        q.ended_at_lte,
        page,
        page_size,
    )
    .await
    .map_err(WebError::Core)?;
    Ok(Json(LogSearchResponse {
        items: rows.into_iter().map(ConnLogResponse::from).collect(),
        total,
        page,
        page_size,
    }))
}

// ── Ban rules CRUD ───────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct BanRuleResponse {
    pub id: Uuid,
    pub scope_type: String,
    pub peer_ip: Option<String>,
    pub user_id: Option<Uuid>,
    pub reason: Option<String>,
    pub active_until: Option<String>,
    pub created_by: Uuid,
    pub action: String,
    pub created_at: String,
    pub updated_at: String,
    pub deleted_at: Option<String>,
}

impl From<BanRule> for BanRuleResponse {
    fn from(r: BanRule) -> Self {
        use time::format_description::well_known::Rfc3339;
        Self {
            id: r.id,
            scope_type: r.scope_type,
            peer_ip: r.peer_ip,
            user_id: r.user_id,
            reason: r.reason,
            active_until: r.active_until.map(|t| t.format(&Rfc3339).unwrap_or_default()),
            created_by: r.created_by,
            action: r.action,
            created_at: r.ts.timestamps.created_at.format(&Rfc3339).unwrap_or_default(),
            updated_at: r.ts.timestamps.updated_at.format(&Rfc3339).unwrap_or_default(),
            deleted_at: r
                .ts
                .soft_delete
                .deleted_at
                .map(|t| t.format(&Rfc3339).unwrap_or_default()),
        }
    }
}

pub async fn list_ban_rules(
    AdminUser(_admin): AdminUser,
    State(state): State<AppState>,
) -> Result<Json<Vec<BanRuleResponse>>, WebError> {
    let rules = BanRule::list_all(&state.db).await.map_err(WebError::Core)?;
    Ok(Json(rules.into_iter().map(BanRuleResponse::from).collect()))
}

#[derive(Deserialize)]
pub struct CreateBanRuleBody {
    pub scope_type: String,
    pub peer_ip: Option<String>,
    pub user_id: Option<Uuid>,
    pub reason: Option<String>,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub active_until: Option<OffsetDateTime>,
    pub action: String,
}

/// Pure validation for a create-ban-rule request — extracted so it's
/// unit-testable without needing a DB/HTTP request.
fn validate_create_ban_rule(body: &CreateBanRuleBody) -> Result<(), WebError> {
    let valid_types = ["peer_ip", "user"];
    if !valid_types.contains(&body.scope_type.as_str()) {
        return Err(WebError::BadRequest("invalid scope_type".into()));
    }
    if !["trap", "ban"].contains(&body.action.as_str()) {
        return Err(WebError::BadRequest("invalid action".into()));
    }
    match body.scope_type.as_str() {
        "peer_ip" if body.peer_ip.is_none() => {
            Err(WebError::BadRequest("peer_ip required for scope_type 'peer_ip'".into()))
        }
        "user" if body.user_id.is_none() => {
            Err(WebError::BadRequest("user_id required for scope_type 'user'".into()))
        }
        _ => Ok(()),
    }
}

pub async fn create_ban_rule(
    AdminUser(admin): AdminUser,
    State(state): State<AppState>,
    Json(body): Json<CreateBanRuleBody>,
) -> Result<(StatusCode, Json<BanRuleResponse>), WebError> {
    validate_create_ban_rule(&body)?;

    let rule = BanRule::create(
        &state.db,
        &body.scope_type,
        body.peer_ip.as_deref(),
        body.user_id,
        body.reason.as_deref(),
        body.active_until,
        admin.id,
        &body.action,
    )
    .await
    .map_err(WebError::Core)?;

    Ok((StatusCode::CREATED, Json(BanRuleResponse::from(rule))))
}

pub async fn delete_ban_rule(
    AdminUser(_admin): AdminUser,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, WebError> {
    if BanRule::soft_delete(&state.db, id).await.map_err(WebError::Core)? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(WebError::NotFound)
    }
}

pub async fn restore_ban_rule(
    AdminUser(_admin): AdminUser,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, WebError> {
    if BanRule::restore(&state.db, id).await.map_err(WebError::Core)? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(WebError::NotFound)
    }
}

// ── Global tarpit enable/disable ─────────────────────────────────────────────

#[derive(Serialize)]
pub struct TarpitSettingsResponse {
    pub enabled: bool,
}

#[derive(Deserialize)]
pub struct UpdateTarpitSettingsBody {
    pub enabled: bool,
}

pub async fn get_tarpit_settings(
    AdminUser(_admin): AdminUser,
    State(state): State<AppState>,
) -> Result<Json<TarpitSettingsResponse>, WebError> {
    let enabled = Settings::get(&state.db, "tarpit_enabled")
        .await
        .map_err(WebError::Core)?
        .map(|v| v == "true")
        .unwrap_or(true);

    Ok(Json(TarpitSettingsResponse { enabled }))
}

pub async fn update_tarpit_settings(
    AdminUser(_admin): AdminUser,
    State(state): State<AppState>,
    Json(body): Json<UpdateTarpitSettingsBody>,
) -> Result<StatusCode, WebError> {
    Settings::set(&state.db, "tarpit_enabled", if body.enabled { "true" } else { "false" })
        .await
        .map_err(WebError::Core)?;
    Ok(StatusCode::NO_CONTENT)
}

// ── Threshold rules CRUD ─────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct TarpitThresholdResponse {
    pub id: Uuid,
    pub fail_count: i32,
    pub window_seconds: i64,
    pub enabled: bool,
    pub action: String,
    pub created_at: String,
    pub updated_at: String,
    pub deleted_at: Option<String>,
}

impl From<TarpitThreshold> for TarpitThresholdResponse {
    fn from(t: TarpitThreshold) -> Self {
        use time::format_description::well_known::Rfc3339;
        Self {
            id: t.id,
            fail_count: t.fail_count,
            window_seconds: t.window_seconds,
            enabled: t.enabled,
            action: t.action,
            created_at: t.ts.timestamps.created_at.format(&Rfc3339).unwrap_or_default(),
            updated_at: t.ts.timestamps.updated_at.format(&Rfc3339).unwrap_or_default(),
            deleted_at: t
                .ts
                .soft_delete
                .deleted_at
                .map(|t| t.format(&Rfc3339).unwrap_or_default()),
        }
    }
}

#[derive(Deserialize)]
pub struct TarpitThresholdBody {
    pub fail_count: i32,
    pub window_seconds: i64,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_trap")]
    pub action: String,
}

fn default_true() -> bool {
    true
}

fn default_trap() -> String {
    "trap".to_string()
}

fn validate_threshold_body(body: &TarpitThresholdBody) -> Result<(), WebError> {
    if body.fail_count <= 0 {
        return Err(WebError::BadRequest("fail_count must be positive".into()));
    }
    if body.window_seconds <= 0 {
        return Err(WebError::BadRequest("window_seconds must be positive".into()));
    }
    if !["trap", "ban"].contains(&body.action.as_str()) {
        return Err(WebError::BadRequest("invalid action".into()));
    }
    Ok(())
}

pub async fn list_tarpit_thresholds(
    AdminUser(_admin): AdminUser,
    State(state): State<AppState>,
) -> Result<Json<Vec<TarpitThresholdResponse>>, WebError> {
    let rules = TarpitThreshold::list_all(&state.db).await.map_err(WebError::Core)?;
    Ok(Json(rules.into_iter().map(TarpitThresholdResponse::from).collect()))
}

pub async fn create_tarpit_threshold(
    AdminUser(_admin): AdminUser,
    State(state): State<AppState>,
    Json(body): Json<TarpitThresholdBody>,
) -> Result<(StatusCode, Json<TarpitThresholdResponse>), WebError> {
    validate_threshold_body(&body)?;
    let rule = TarpitThreshold::create(
        &state.db,
        body.fail_count,
        body.window_seconds,
        body.enabled,
        &body.action,
    )
    .await
    .map_err(WebError::Core)?;
    Ok((StatusCode::CREATED, Json(TarpitThresholdResponse::from(rule))))
}

pub async fn update_tarpit_threshold(
    AdminUser(_admin): AdminUser,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<TarpitThresholdBody>,
) -> Result<Json<TarpitThresholdResponse>, WebError> {
    validate_threshold_body(&body)?;
    let rule = TarpitThreshold::update(
        &state.db,
        id,
        body.fail_count,
        body.window_seconds,
        body.enabled,
        &body.action,
    )
    .await
    .map_err(WebError::Core)?
    .ok_or(WebError::NotFound)?;
    Ok(Json(TarpitThresholdResponse::from(rule)))
}

pub async fn delete_tarpit_threshold(
    AdminUser(_admin): AdminUser,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, WebError> {
    if TarpitThreshold::soft_delete(&state.db, id).await.map_err(WebError::Core)? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(WebError::NotFound)
    }
}

pub async fn restore_tarpit_threshold(
    AdminUser(_admin): AdminUser,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, WebError> {
    if TarpitThreshold::restore(&state.db, id).await.map_err(WebError::Core)? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(WebError::NotFound)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(scope_type: &str, peer_ip: Option<&str>, user_id: Option<Uuid>) -> CreateBanRuleBody {
        CreateBanRuleBody {
            scope_type: scope_type.to_string(),
            peer_ip: peer_ip.map(String::from),
            user_id,
            reason: None,
            active_until: None,
            action: "ban".to_string(),
        }
    }

    #[test]
    fn rejects_unknown_scope_type() {
        let err = validate_create_ban_rule(&body("hostname", None, None)).unwrap_err();
        assert!(matches!(err, WebError::BadRequest(_)));
    }

    #[test]
    fn peer_ip_scope_requires_peer_ip() {
        assert!(validate_create_ban_rule(&body("peer_ip", None, None)).is_err());
        assert!(validate_create_ban_rule(&body("peer_ip", Some("203.0.113.1"), None)).is_ok());
    }

    #[test]
    fn user_scope_requires_user_id() {
        assert!(validate_create_ban_rule(&body("user", None, None)).is_err());
        assert!(validate_create_ban_rule(&body("user", None, Some(Uuid::now_v7()))).is_ok());
    }

    #[test]
    fn rejects_unknown_ban_rule_action() {
        let mut b = body("peer_ip", Some("203.0.113.1"), None);
        b.action = "smite".to_string();
        assert!(validate_create_ban_rule(&b).is_err());
    }

    #[test]
    fn rejects_unknown_threshold_action() {
        let body = TarpitThresholdBody {
            fail_count: 5,
            window_seconds: 60,
            enabled: true,
            action: "smite".to_string(),
        };
        assert!(validate_threshold_body(&body).is_err());
    }

    #[test]
    fn pagination_defaults_and_clamps() {
        assert_eq!(normalize_pagination(None, None), (1, 50));
        assert_eq!(normalize_pagination(Some(0), Some(0)), (1, 1));
        assert_eq!(normalize_pagination(Some(-5), Some(9999)), (1, 200));
        assert_eq!(normalize_pagination(Some(3), Some(20)), (3, 20));
    }
}
