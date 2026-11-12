//! A runner's socket at the edge (`crates/runner-protocol/src/wire/mod.rs`),
//! which `demi_core_api::runner_socket::accept` serves: a known token moves
//! the socket into the core of the device's owner, and a runner without one
//! waits for its user to claim the code it printed.

use std::sync::Arc;

use demi_backend_database::accounts::TokenHash;
use demi_backend_user_shard::services::Services;
use demi_core::pool::Shards;
use demi_core_api::runner_socket::{Presented, accept as serve};
use demi_core_devices::devices::RunnerSocket;
use demi_core_store::StorageError;

/// Serves a runner's socket until it is handed to a core or closes: its
/// token finds its device in the control database, and the device of a
/// disabled account is turned away without a word, so enabling the account
/// brings its runners back.
pub(crate) async fn accept(services: Arc<Services>, shards: Shards, socket: RunnerSocket) {
    let find = |token: TokenHash| async {
        let Some((id, owner)) = services.control.device_by_token(token).await? else {
            return Ok(Presented::Unknown);
        };
        let Some(device) = services.stores.of(&owner).await?.device(id).await? else {
            return Ok(Presented::Unknown);
        };
        let account = services.control.account(owner.clone()).await?;
        if !account.is_some_and(|account| !account.user.disabled) {
            tracing::info!(event = "runner_connection.closed", device_id = %device.id, reason = "account_disabled", "closing a runner connection of a disabled account");
            return Ok(Presented::Away);
        }
        Ok::<_, StorageError>(Presented::Device(owner, device))
    };
    serve(socket, &services.runners, &services.claims, &shards, find).await;
}
