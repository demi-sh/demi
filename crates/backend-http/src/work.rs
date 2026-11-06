//! The work's route: a request of changes, which the user's core applies
//! whole or not at all and answers with the work's revision; the
//! conversation's entry in the list carries the tabs.
//!
//! The backend applies the changes of a conversation's work one at a time, the page's
//! and the plugin's alike. A plugin whose work on one conversation's tabs takes
//! several port operations, such as reading the browser's tabs and then adding
//! the missing ones, does that work one piece at a time per conversation, so a
//! change it makes is never based on what it read before its own earlier
//! change.
//!
//! The user creates a tab of a plugin's kind only through an intent the kind
//! opens; otherwise the plugin creates them.


use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use demi_plugin_host::WorkError;
use demi_web_api_protocol::error::ErrorCode;
use demi_web_api_protocol::work::{WORK_BYTES_MAX, WorkAnswer, WorkChanges};

use super::AppState;
use super::body::JsonBody;
use super::conversations::owned;
use super::error::ApiError;
use super::gate::AuthUser;

/// `POST /conversations/:id/work/changes { changes }`: applied in order,
/// all or none, as one change of the work.
pub(super) async fn change(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(id): Path<String>,
    body: Result<JsonBody<WorkChanges>, ApiError>,
) -> Result<Json<WorkAnswer>, ApiError> {
    let conversation = owned(&state.services, &user.id, &id).await?.id;
    let JsonBody(WorkChanges { changes }) = body?;
    let (revision, changed) = state
        .shards
        .of(&user.id)
        .call(move |core, _| async move {
            core.plugins().change_work(conversation, changes).await
        })
        .await?
        .map_err(refused)?;
    Ok(Json(WorkAnswer { revision, changed }))
}

fn refused(error: WorkError) -> ApiError {
    let message = error.to_string();
    match error {
        WorkError::UnknownKind(_) => ApiError::new(
            StatusCode::BAD_REQUEST,
            ErrorCode::UnknownTabKind,
            message,
        ),
        WorkError::Full => ApiError::new(StatusCode::CONFLICT, ErrorCode::WorkFull, message),
        WorkError::TooLarge => {
            ApiError::new(StatusCode::PAYLOAD_TOO_LARGE, ErrorCode::TooLarge, message)
                .with_param("limitBytes", WORK_BYTES_MAX)
        }
        WorkError::Missing => ApiError::new(
            StatusCode::NOT_FOUND,
            ErrorCode::ConversationNotFound,
            message,
        ),
        WorkError::Storage(error) => error.into(),
    }
}
