use axum::http::StatusCode;

use crate::extractors::AdminUser;

/// Deliberately panics so a Sentry/Bugsink triage can confirm the backend
/// side of error reporting works end to end. Admin-gated so it isn't a
/// public panic endpoint.
pub async fn sample_error(AdminUser(_admin): AdminUser) -> StatusCode {
    panic!("backend sample error for Sentry/Bugsink verification");
}
