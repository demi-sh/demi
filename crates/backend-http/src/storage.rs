//! `/api/storage`: what Jush keeps on the server and on the caller's devices,
//! the rules that clean it up, and the cleanups the caller makes by hand
//! (`crates/backend-user-shard/src/shard/storage.rs`).
//!
//! A device's request reaches its runner, which takes minutes to walk its
//! files; a device whose runner is not connected answers 409
//! `device_offline`.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use jush_backend_user_shard::shard::storage::StorageRequestError;
use jush_web_api_protocol::auth::Role;
use jush_web_api_protocol::error::ErrorCode;
use jush_web_api_protocol::ids::DeviceId;
use jush_web_api_protocol::storage::{DeviceStorageReport, LogRetention, Storage, StorageAction, StorageRules};

use super::AppState;
use super::body::JsonBody;
use super::error::ApiError;
use super::gate::{AdminUser, AuthUser};

pub(super) async fn read(State(state): State<AppState>, AuthUser(user): AuthUser) -> Result<Json<Storage>, ApiError> {
    let administrator = user.role.outranks(Role::User);
    let storage = state
        .shards
        .of(&user.id)
        .call(move |shard, _| async move { shard.storage(administrator).await })
        .await??;
    Ok(Json(storage))
}

pub(super) async fn set_rules(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    JsonBody(rules): JsonBody<StorageRules>,
) -> Result<Json<StorageRules>, ApiError> {
    let rules = state
        .shards
        .of(&user.id)
        .call(move |shard, _| async move { shard.set_storage_rules(rules).await })
        .await??;
    Ok(Json(rules))
}

pub(super) async fn set_log(
    State(state): State<AppState>,
    AdminUser(user): AdminUser,
    JsonBody(retention): JsonBody<LogRetention>,
) -> Result<Json<LogRetention>, ApiError> {
    let retention = state
        .shards
        .of(&user.id)
        .call(move |shard, _| async move { shard.set_log_retention(retention).await })
        .await??;
    Ok(Json(retention))
}

pub(super) async fn device(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(device): Path<DeviceId>,
    JsonBody(action): JsonBody<StorageAction>,
) -> Result<Json<DeviceStorageReport>, ApiError> {
    let answered = state
        .shards
        .of(&user.id)
        // The caller's shard holds only the caller's devices, so another
        // user's device is as unknown as an offline one.
        .call(move |shard, _| async move { shard.device_storage(&device, action).await })
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
