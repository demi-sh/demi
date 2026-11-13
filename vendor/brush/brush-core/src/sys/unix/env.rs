//! Environment variable retrieval for Unix platforms.

/// Retrieves environment variables from the host process, as the shell's
/// text (`bytes::from_os`), where [`std::env::vars()`] panics at a byte that
/// is no UTF-8.
pub fn get_host_env_vars() -> impl Iterator<Item = (String, String)> {
    std::env::vars_os().map(|(name, value)| {
        (
            crate::bytes::from_os(&name).into_owned(),
            crate::bytes::from_os(&value).into_owned(),
        )
    })
}
