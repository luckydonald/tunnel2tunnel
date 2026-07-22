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
    pub q: Option<String>,
}

#[derive(Serialize)]
pub struct LogSearchResponse {
    pub items: Vec<ConnLogResponse>,
    pub total: i64,
    pub page: i64,
    pub page_size: i64,
}

pub async fn search_connection_logs(
    AdminUser(_admin): AdminUser,
    State(state): State<AppState>,
    Query(q): Query<LogSearchQuery>,
) -> Result<Json<LogSearchResponse>, WebError> {
    let page = q.page.unwrap_or(1).max(1);
    let page_size = q.page_size.unwrap_or(50).clamp(1, 200);
    let (rows, total) = ConnectionLog::search(
        &state.db,
        q.peer_ip.as_deref(),
        q.user_id,
        q.success,
        q.method.as_deref(),
        q.q.as_deref(),
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
    pub created_at: String,
    pub updated_at: String,
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
            created_at: r.ts.created_at.format(&Rfc3339).unwrap_or_default(),
            updated_at: r.ts.updated_at.format(&Rfc3339).unwrap_or_default(),
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
}

pub async fn create_ban_rule(
    AdminUser(admin): AdminUser,
    State(state): State<AppState>,
    Json(body): Json<CreateBanRuleBody>,
) -> Result<(StatusCode, Json<BanRuleResponse>), WebError> {
    let valid_types = ["peer_ip", "user"];
    if !valid_types.contains(&body.scope_type.as_str()) {
        return Err(WebError::BadRequest("invalid scope_type".into()));
    }
    match body.scope_type.as_str() {
        "peer_ip" if body.peer_ip.is_none() => {
            return Err(WebError::BadRequest("peer_ip required for scope_type 'peer_ip'".into()));
        }
        "user" if body.user_id.is_none() => {
            return Err(WebError::BadRequest("user_id required for scope_type 'user'".into()));
        }
        _ => {}
    }

    let rule = BanRule::create(
        &state.db,
        &body.scope_type,
        body.peer_ip.as_deref(),
        body.user_id,
        body.reason.as_deref(),
        body.active_until,
        admin.id,
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
    if BanRule::delete(&state.db, id).await.map_err(WebError::Core)? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(WebError::NotFound)
    }
}

// ── Global threshold settings ────────────────────────────────────────────────

#[derive(Serialize)]
pub struct TarpitSettingsResponse {
    pub threshold_count: u32,
    pub threshold_window_seconds: u64,
    pub enabled: bool,
}

#[derive(Deserialize)]
pub struct UpdateTarpitSettingsBody {
    pub threshold_count: u32,
    pub threshold_window_seconds: u64,
    pub enabled: bool,
}

pub async fn get_tarpit_settings(
    AdminUser(_admin): AdminUser,
    State(state): State<AppState>,
) -> Result<Json<TarpitSettingsResponse>, WebError> {
    let threshold_count = Settings::get(&state.db, "tarpit_threshold_count")
        .await
        .map_err(WebError::Core)?
        .and_then(|v| v.parse().ok())
        .unwrap_or(5);
    let threshold_window_seconds = Settings::get(&state.db, "tarpit_threshold_window_seconds")
        .await
        .map_err(WebError::Core)?
        .and_then(|v| v.parse().ok())
        .unwrap_or(600);
    let enabled = Settings::get(&state.db, "tarpit_enabled")
        .await
        .map_err(WebError::Core)?
        .map(|v| v == "true")
        .unwrap_or(true);

    Ok(Json(TarpitSettingsResponse {
        threshold_count,
        threshold_window_seconds,
        enabled,
    }))
}

pub async fn update_tarpit_settings(
    AdminUser(_admin): AdminUser,
    State(state): State<AppState>,
    Json(body): Json<UpdateTarpitSettingsBody>,
) -> Result<StatusCode, WebError> {
    if body.threshold_count == 0 {
        return Err(WebError::BadRequest("threshold_count must be positive".into()));
    }
    if body.threshold_window_seconds == 0 {
        return Err(WebError::BadRequest("threshold_window_seconds must be positive".into()));
    }
    Settings::set(&state.db, "tarpit_threshold_count", &body.threshold_count.to_string())
        .await
        .map_err(WebError::Core)?;
    Settings::set(
        &state.db,
        "tarpit_threshold_window_seconds",
        &body.threshold_window_seconds.to_string(),
    )
    .await
    .map_err(WebError::Core)?;
    Settings::set(&state.db, "tarpit_enabled", if body.enabled { "true" } else { "false" })
        .await
        .map_err(WebError::Core)?;
    Ok(StatusCode::NO_CONTENT)
}
