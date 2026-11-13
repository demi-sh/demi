//! The session cookie.
//!
//! It is named `jush_session` and is `HttpOnly`, `SameSite=Lax` and `Path=/`.
//! It is `Secure` for a request that came over HTTPS, directly or through a
//! proxy.
//!
//! A browser signed in before the product took the name Jush sends the token
//! as `demi_session`. The backend reads it as the session cookie, and the
//! session's gate replaces it with `jush_session`, so nobody signs in again.
//! The earlier name goes with the other migrations when the history starts
//! again at the first formal release.

use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::http::uri::Scheme;
use axum::http::{HeaderMap, Uri};
use axum_extra::extract::CookieJar;
use axum_extra::extract::cookie::{Cookie, Expiration, SameSite};
use cookie::time::OffsetDateTime;
use jush_shared_types::Timestamp;

pub(super) const SESSION_COOKIE: &str = "jush_session";

const EARLIER_SESSION_COOKIE: &str = "demi_session";

/// The session token the request's cookies carry, and whether under the name
/// of before the rename.
pub(super) fn session_token(jar: &CookieJar) -> Option<(String, bool)> {
    match jar.get(SESSION_COOKIE) {
        Some(cookie) => Some((cookie.value().to_owned(), false)),
        None => jar
            .get(EARLIER_SESSION_COOKIE)
            .map(|cookie| (cookie.value().to_owned(), true)),
    }
}

/// The jar with the cookie of before the rename removed.
pub(super) fn remove_earlier(jar: CookieJar, https: bool) -> CookieJar {
    jar.remove(
        Cookie::build((EARLIER_SESSION_COOKIE, String::new()))
            .http_only(true)
            .same_site(SameSite::Lax)
            .path("/")
            .secure(https)
            .build(),
    )
}

/// The cookie that carries a session's token until the session expires.
pub(super) fn session(token: &str, expires_at: Timestamp, https: bool) -> Cookie<'static> {
    let expires = OffsetDateTime::from_unix_timestamp(expires_at.to_jiff().as_second())
        .expect("every jiff timestamp is in the cookie time range");
    base(token.to_owned(), https)
        .expires(Expiration::DateTime(expires))
        .build()
}

/// The jar with the session cookie removed, so the user's browser stops
/// sending it.
pub(super) fn remove(jar: CookieJar, https: bool) -> CookieJar {
    jar.remove(base(String::new(), https).build())
}

fn base(value: String, https: bool) -> cookie::CookieBuilder<'static> {
    Cookie::build((SESSION_COOKIE, value))
        .http_only(true)
        .same_site(SameSite::Lax)
        .path("/")
        .secure(https)
}

/// Whether the request came over HTTPS: by its own scheme, or by the first
/// value of `X-Forwarded-Proto`.
pub(super) fn over_https(uri: &Uri, headers: &HeaderMap) -> bool {
    uri.scheme() == Some(&Scheme::HTTPS)
        || headers
            .get("x-forwarded-proto")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.split(',').next())
            .is_some_and(|first| first.trim().eq_ignore_ascii_case("https"))
}

/// Whether the request came over HTTPS, as a handler's argument.
pub(super) struct Https(pub(super) bool);

impl<S: Send + Sync> FromRequestParts<S> for Https {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, Self::Rejection> {
        Ok(Self(over_https(&parts.uri, &parts.headers)))
    }
}
