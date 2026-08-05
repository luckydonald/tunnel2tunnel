use axum::{extract::State, http::StatusCode, Json};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use tunnel2tunnel_core::{
    auth::verify_password,
    models::{entity::Entity, entity_access::EntityAccess, port_config::PortConfig, ssh_key::SshKey},
};

use crate::{error::WebError, extractors::AuthUser, AppState};

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
    if !verify_password(&body.old_password, &user.password_hash).map_err(WebError::Core)? {
        return Err(WebError::BadRequest("incorrect current password".into()));
    }
    if body.new_password.len() < 8 {
        return Err(WebError::BadRequest(
            "password must be at least 8 characters".into(),
        ));
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
    pub entity_name: Option<String>,
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
    let entities = Entity::list_for_user(&state.db, user.id)
        .await
        .map_err(WebError::Core)?;
    let entity_names: std::collections::HashMap<Uuid, Option<String>> =
        entities.into_iter().map(|e| (e.id, e.name)).collect();
    Ok(Json(
        keys.into_iter()
            .map(|k| KeySummary {
                entity_name: entity_names.get(&k.entity_id).cloned().flatten(),
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
    let owned_ids: std::collections::HashSet<Uuid> = all_keys.iter().map(|k| k.id).collect();

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
    pub owner_entity_name: Option<String>,
    pub subject_type: String,
    pub subject_entity_id: Option<Uuid>,
    pub subject_entity_name: Option<String>,
    pub subject_user_id: Option<Uuid>,
    pub hostname: Option<String>,
    pub port_config_id: Option<Uuid>,
    pub port_config_name: Option<String>,
}

pub async fn list_my_access(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
) -> Result<Json<Vec<AccessSummary>>, WebError> {
    let rules = EntityAccess::list_for_user(&state.db, user.id)
        .await
        .map_err(WebError::Core)?;

    let owned_entities = Entity::list_for_user(&state.db, user.id)
        .await
        .map_err(WebError::Core)?;
    let owner_names: std::collections::HashMap<Uuid, Option<String>> =
        owned_entities.into_iter().map(|e| (e.id, e.name)).collect();

    // `subject_entity_id` can point at a friend's entity (not in `owned_entities`) —
    // resolved one at a time since there's no batch-by-ids lookup and rule counts
    // on this admin-facing page are small.
    let mut subject_entity_names: std::collections::HashMap<Uuid, Option<String>> =
        std::collections::HashMap::new();
    let mut port_names: std::collections::HashMap<Uuid, Option<String>> =
        std::collections::HashMap::new();
    for r in &rules {
        if let Some(sid) = r.subject_entity_id {
            if !subject_entity_names.contains_key(&sid) {
                let name = Entity::find_by_id_only(&state.db, sid)
                    .await
                    .map_err(WebError::Core)?
                    .and_then(|e| e.name);
                subject_entity_names.insert(sid, name);
            }
        }
        if let Some(pid) = r.port_config_id {
            if !port_names.contains_key(&pid) {
                let name = PortConfig::find_by_id(&state.db, pid)
                    .await
                    .map_err(WebError::Core)?
                    .map(|p| p.name);
                port_names.insert(pid, name);
            }
        }
    }

    Ok(Json(
        rules
            .into_iter()
            .map(|r| AccessSummary {
                owner_entity_name: owner_names.get(&r.owner_entity_id).cloned().flatten(),
                subject_entity_name: r
                    .subject_entity_id
                    .and_then(|sid| subject_entity_names.get(&sid).cloned().flatten()),
                port_config_name: r
                    .port_config_id
                    .and_then(|pid| port_names.get(&pid).cloned().flatten()),
                id: r.id,
                owner_entity_id: r.owner_entity_id,
                subject_type: r.subject_type,
                subject_entity_id: r.subject_entity_id,
                subject_user_id: r.subject_user_id,
                hostname: r.hostname,
                port_config_id: r.port_config_id,
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
