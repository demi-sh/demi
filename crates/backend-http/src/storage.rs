//! `/api/storage`: what Demi keeps on the server and on the caller's devices,
//! the rules that clean it up, and the cleanups the caller makes by hand. The
//! caller's core answers for its devices and conversations
//! (`crates/core/src/storage.rs`); the server adds what it keeps
//! (`crates/backend-user-shard/src/server_storage.rs`).
//!
//! A device's request reaches its runner, which takes minutes to walk its
//! files; a device whose runner is not connected answers 409
//! `device_offline`.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use demi_core::storage::StorageRequestError;
use demi_web_api_protocol::auth::Role;
use demi_web_api_protocol::error::ErrorCode;
use demi_web_api_protocol::ids::DeviceId;
use demi_web_api_protocol::storage::{
    DeviceStorageReport, LogRetention, ServerStorage, Storage, StorageAction, StorageRules,
};

use demi_core_api::body::JsonBody;
use demi_core_api::error::ApiError;

use super::AppState;
use super::gate::{AdminUser, AuthUser};

/// What the server and each of the caller's devices keep, and the rules. An
/// administrator sees the whole server; anyone else sees their own.
pub(super) async fn read(State(state): State<AppState>, AuthUser(user): AuthUser) -> Result<Json<Storage>, ApiError> {
    let administrator = user.role.outranks(Role::User);
    let core = state
        .api
        .shards
        .of(&user.id)
        .call(move |core, _| async move { core.storage().await })
        .await??;
    let services = &state.services;
    let measured = services.server_storage.measured(services).await;
    let server = if administrator {
        ServerStorage {
            measured_at: measured.at,
            disk_bytes: measured.disk.map(|(total, _)| total),
            disk_available_bytes: measured.disk.map(|(_, available)| available),
            conversations_bytes: measured.conversations.values().sum(),
            attachments_bytes: measured.attachments.values().sum(),
            clouds_bytes: measured.clouds,
            log_bytes: Some(measured.log),
            search_bytes: Some(measured.search),
            log: Some(services.control.log_retention().await?),
        }
    } else {
        ServerStorage {
            measured_at: measured.at,
            disk_bytes: None,
            disk_available_bytes: None,
            conversations_bytes: core
                .owned
                .iter()
                .filter_map(|id| measured.conversations.get(id.as_str()))
                .sum(),
            attachments_bytes: measured.attachments.get(user.id.as_str()).copied().unwrap_or(0),
            clouds_bytes: None,
            log_bytes: None,
            search_bytes: None,
            log: None,
        }
    };
    Ok(Json(Storage {
        server,
        rules: core.rules,
        devices: core.devices,
        conversations: core.conversations,
    }))
}

pub(super) async fn set_rules(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    JsonBody(rules): JsonBody<StorageRules>,
) -> Result<Json<StorageRules>, ApiError> {
    let rules = state
        .api
        .shards
        .of(&user.id)
        .call(move |core, _| async move { core.set_storage_rules(rules).await })
        .await??;
    Ok(Json(rules))
}

/// Keeps how long and how much of its log the server keeps, and applies it
/// from the log's next trim.
pub(super) async fn set_log(
    State(state): State<AppState>,
    AdminUser(_): AdminUser,
    JsonBody(retention): JsonBody<LogRetention>,
) -> Result<Json<LogRetention>, ApiError> {
    let services = &state.services;
    services.control.set_log_retention(retention).await?;
    services.log.set_limits(retention.into());
    Ok(Json(retention))
}

pub(super) async fn device(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(device): Path<DeviceId>,
    JsonBody(action): JsonBody<StorageAction>,
) -> Result<Json<DeviceStorageReport>, ApiError> {
    let answered = state
        .api
        .shards
        .of(&user.id)
        // The caller's core holds only the caller's devices, so another
        // user's device is as unknown as an offline one.
        .call(move |core, _| async move { core.device_storage(&device, action).await })
        .await?;
    match answered {
        Ok(report) => Ok(Json(report)),
        Err(StorageRequestError::Offline) => Err(ApiError::device_offline()),
        Err(StorageRequestError::UnknownConversation) => Err(ApiError::new(
            StatusCode::NOT_FOUND,
            ErrorCode::ConversationNotFound,
            "No such conversation",
        )),
        Err(StorageRequestError::Device(error)) => Err(ApiError::device_operation(error)),
        Err(StorageRequestError::Storage(error)) => Err(error.into()),
    }
}
