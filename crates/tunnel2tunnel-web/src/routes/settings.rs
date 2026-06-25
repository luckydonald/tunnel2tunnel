use axum::{
    extract::State,
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use tunnel2tunnel_core::{
    auth::verify_password,
    models::{
        entity_access::EntityAccess,
        ssh_key::SshKey,
    },
};

use crate::{extractors::AuthUser, error::WebError, AppState};

// ── Change password ────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct ChangePasswordBody {
    pub old_password: String,
    pub new_password: String,
}

pub async fn change_password(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Json(body): Json<ChangePasswordBody>,
) -> Result<StatusCode, WebError> {
    if !verify_password(&body.old_password, &user.password_hash)
        .map_err(WebError::Core)?
    {
        return Err(WebError::BadRequest("incorrect current password".into()));
    }
    if body.new_password.len() < 8 {
        return Err(WebError::BadRequest("password must be at least 8 characters".into()));
    }

    use tunnel2tunnel_core::models::user::User;
    User::update_password(&state.db, user.id, &body.new_password)
        .await
        .map_err(WebError::Core)?;

    Ok(StatusCode::NO_CONTENT)
}

// ── SSH key management (purge) ─────────────────────────────────────────────────

#[derive(Serialize)]
pub struct KeySummary {
    pub id: Uuid,
    pub entity_id: Uuid,
    pub algorithm: String,
    pub fingerprint: String,
    pub name: Option<String>,
    pub comment: Option<String>,
}

pub async fn list_my_keys(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
) -> Result<Json<Vec<KeySummary>>, WebError> {
    let keys = SshKey::list_for_user(&state.db, user.id)
        .await
        .map_err(WebError::Core)?;
    Ok(Json(
        keys.into_iter()
            .map(|k| KeySummary {
                id: k.id,
                entity_id: k.entity_id,
                algorithm: k.algorithm,
                fingerprint: k.fingerprint,
                name: k.name,
                comment: k.comment,
            })
            .collect(),
    ))
}

#[derive(Deserialize)]
pub struct PurgeKeysBody {
    pub ids: Vec<Uuid>,
}

pub async fn purge_keys(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Json(body): Json<PurgeKeysBody>,
) -> Result<StatusCode, WebError> {
    // For each key, verify ownership via entity FK, then soft-delete
    let all_keys = SshKey::list_for_user(&state.db, user.id)
        .await
        .map_err(WebError::Core)?;
    let owned_ids: std::collections::HashSet<Uuid> =
        all_keys.iter().map(|k| k.id).collect();

    for id in &body.ids {
        if !owned_ids.contains(id) {
            return Err(WebError::Forbidden);
        }
        // Find entity_id for this key
        if let Some(key) = all_keys.iter().find(|k| k.id == *id) {
            SshKey::soft_delete(&state.db, *id, key.entity_id)
                .await
                .map_err(WebError::Core)?;
        }
    }

    Ok(StatusCode::NO_CONTENT)
}

// ── Access rule management (purge) ────────────────────────────────────────────

#[derive(Serialize)]
pub struct AccessSummary {
    pub id: Uuid,
    pub owner_entity_id: Uuid,
    pub subject_type: String,
    pub subject_entity_id: Option<Uuid>,
    pub subject_user_id: Option<Uuid>,
    pub hostname: Option<String>,
}

pub async fn list_my_access(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
) -> Result<Json<Vec<AccessSummary>>, WebError> {
    let rules = EntityAccess::list_for_user(&state.db, user.id)
        .await
        .map_err(WebError::Core)?;
    Ok(Json(
        rules
            .into_iter()
            .map(|r| AccessSummary {
                id: r.id,
                owner_entity_id: r.owner_entity_id,
                subject_type: r.subject_type,
                subject_entity_id: r.subject_entity_id,
                subject_user_id: r.subject_user_id,
                hostname: r.hostname,
            })
            .collect(),
    ))
}

#[derive(Deserialize)]
pub struct PurgeAccessBody {
    pub ids: Vec<Uuid>,
}

pub async fn purge_access(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Json(body): Json<PurgeAccessBody>,
) -> Result<StatusCode, WebError> {
    let all_rules = EntityAccess::list_for_user(&state.db, user.id)
        .await
        .map_err(WebError::Core)?;
    let owned: std::collections::HashMap<Uuid, Uuid> = all_rules
        .iter()
        .map(|r| (r.id, r.owner_entity_id))
        .collect();

    for id in &body.ids {
        let entity_id = owned.get(id).ok_or(WebError::Forbidden)?;
        EntityAccess::delete(&state.db, *id, *entity_id)
            .await
            .map_err(WebError::Core)?;
    }

    Ok(StatusCode::NO_CONTENT)
}
