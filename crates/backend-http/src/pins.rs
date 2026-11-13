//! `DELETE /conversations/:id/pins/:pin`: the user unpins a document the
//! conversation's main agent pinned
//! (`crates/backend-user-shard/src/conversation/pins.rs`), which the agent
//! learns before its next request. A document no longer pinned answers as
//! one just unpinned.

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use jush_backend_database::pins::Unpinner;
use jush_backend_page_sync::Part;
use jush_backend_user_shard::services::Services;

use super::conversations::owned;
use super::error::ApiError;
use super::gate::AuthUser;

pub(super) async fn unpin(
    State(services): State<Arc<Services>>,
    AuthUser(user): AuthUser,
    Path((id, pin)): Path<(String, u64)>,
) -> Result<StatusCode, ApiError> {
    let record = owned(&services, &user.id, &id).await?;
    if services.control.unpin(record.id.clone(), pin, Unpinner::User).await? {
        services.sync.mark(&user.id, Part::Metadata(record.id));
    }
    Ok(StatusCode::NO_CONTENT)
}
