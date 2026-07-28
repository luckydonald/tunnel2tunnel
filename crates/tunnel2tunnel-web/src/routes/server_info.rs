use crate::extractors::AuthUser;
use crate::{AppState, WebError};
use axum::{extract::State, Json};
use serde::Serialize;

#[derive(Serialize)]
pub struct ServerInfoResponse {
    pub ssh_port: u16,
    pub ssh_port_env_var: &'static str,
    pub ssh_host_key_fingerprint: String,
}

pub async fn get_server_info(
    AuthUser(_user): AuthUser,
    State(state): State<AppState>,
) -> Result<Json<ServerInfoResponse>, WebError> {
    Ok(Json(ServerInfoResponse {
        ssh_port: state.ssh_port,
        ssh_port_env_var: "SSH_PORT",
        ssh_host_key_fingerprint: state.ssh_host_key_fingerprint.clone(),
    }))
}
