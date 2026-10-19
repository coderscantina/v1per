//! The one error type handlers return. It renders as the protocol's `ApiErrorBody`.

use std::collections::BTreeMap;

use axum::Json;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use v1per_protocol::errors::{ApiErrorBody, ErrorCode, FieldError, FieldErrorCode};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiError {
    body: ApiErrorBody,
}

impl ApiError {
    pub fn new(code: ErrorCode) -> Self {
        Self::with_message(code, default_message(code))
    }

    pub fn with_message(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            body: ApiErrorBody {
                error_code: code,
                message: message.into(),
                errors: None,
                current: None,
            },
        }
    }

    /// Missing, hidden and unknown all answer with exactly this value.
    pub fn not_found() -> Self {
        Self::new(ErrorCode::NotFound)
    }

    /// `stale_revision` carrying the current head.
    pub fn stale_revision(current: i64) -> Self {
        Self::new(ErrorCode::StaleRevision).with_current(current)
    }

    pub fn with_current(mut self, current: i64) -> Self {
        self.body.current = Some(current);
        self
    }

    /// Logs the cause and answers a body without internals.
    pub fn internal(cause: impl std::fmt::Display) -> Self {
        tracing::error!(error = %cause, "internal error");
        Self::new(ErrorCode::Internal)
    }

    pub fn code(&self) -> ErrorCode {
        self.body.error_code
    }

    pub fn body(&self) -> &ApiErrorBody {
        &self.body
    }
}

fn default_message(code: ErrorCode) -> &'static str {
    match code {
        ErrorCode::Unauthenticated => "Sign in to continue.",
        ErrorCode::InvalidCredentials => "Email or password is wrong.",
        ErrorCode::SecondFactorRequired => "Enter the code from your authenticator app.",
        ErrorCode::SecondFactorInvalid => "The code is wrong or was already used.",
        ErrorCode::SecondFactorEnrollmentRequired => {
            "Set up two-factor authentication to continue."
        }
        ErrorCode::ReauthenticationRequired => "Confirm your password to continue.",
        ErrorCode::NotFound => "Not found.",
        ErrorCode::Forbidden => "Your role does not allow this.",
        ErrorCode::OperationNotGranted => "API keys cannot call this operation.",
        ErrorCode::GrantExceedsPrincipal => "The key would hold more than you do on this node.",
        ErrorCode::OriginRejected => {
            "Commands need the request marker and a same-origin Origin header."
        }
        ErrorCode::StaleRevision => "The resource changed since it was read.",
        ErrorCode::IdempotencyConflict => "The idempotency key was used for a different request.",
        ErrorCode::NodeNotEmpty => "The node has descendants. Confirm their number to delete them.",
        ErrorCode::MalformedRequest => "The request does not parse.",
        ErrorCode::InvalidArguments => "Some arguments are invalid.",
        ErrorCode::PayloadTooLarge => "The request is too large.",
        ErrorCode::RateLimited => "Too many attempts. Try again in a minute.",
        ErrorCode::QueryTimeout => "The query took too long and was cancelled.",
        ErrorCode::Internal => "Internal error.",
    }
}

impl From<sqlx::Error> for ApiError {
    fn from(error: sqlx::Error) -> Self {
        Self::internal(error)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = StatusCode::from_u16(self.body.error_code.status())
            .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        let mut response = (status, Json(self.body)).into_response();
        response
            .headers_mut()
            .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
        response
    }
}

/// Field errors collected while validating arguments, answered as `invalid_arguments`.
#[derive(Default)]
pub struct FieldErrors(BTreeMap<String, Vec<FieldError>>);

impl FieldErrors {
    pub fn add(&mut self, field: &str, code: FieldErrorCode, limit: Option<u32>) {
        self.0
            .entry(field.to_owned())
            .or_default()
            .push(FieldError { code, limit });
    }

    /// `invalid_arguments` naming one field.
    pub fn one(field: &str, code: FieldErrorCode, limit: Option<u32>) -> ApiError {
        let mut errors = Self::default();
        errors.add(field, code, limit);
        errors.finish().expect_err("one field error")
    }

    pub fn finish(self) -> Result<(), ApiError> {
        if self.0.is_empty() {
            return Ok(());
        }
        let mut error = ApiError::new(ErrorCode::InvalidArguments);
        error.body.errors = Some(self.0);
        Err(error)
    }
}
