//! The closed set of API error codes and the error body every non-2xx answer carries.
//!
//! Screens branch on `error_code`, never on `message`.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

macro_rules! error_codes {
    ($( $(#[doc = $doc:literal])* $variant:ident = $code:literal, $status:literal; )*) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS, JsonSchema)]
        pub enum ErrorCode {
            $( $(#[doc = $doc])* #[serde(rename = $code)] $variant, )*
        }

        impl ErrorCode {
            /// Every code, in declaration order.
            pub const ALL: &'static [ErrorCode] = &[$(ErrorCode::$variant),*];

            pub fn as_str(self) -> &'static str {
                match self { $(ErrorCode::$variant => $code,)* }
            }

            /// The HTTP status the code is always sent with.
            pub fn status(self) -> u16 {
                match self { $(ErrorCode::$variant => $status,)* }
            }
        }
    };
}

error_codes! {
    /// No valid credential. Sent whether or not the resource exists.
    Unauthenticated = "unauthenticated", 401;
    /// Wrong password or unknown account. Both answer identically.
    InvalidCredentials = "invalid_credentials", 401;
    /// The password was correct; a second factor is needed next.
    SecondFactorRequired = "second_factor_required", 401;
    /// A wrong, used or unreadable time-based or recovery code.
    SecondFactorInvalid = "second_factor_invalid", 401;
    /// The session may only enroll a second factor: a node the person holds a grant on, or an
    /// ancestor of it, requires 2FA.
    SecondFactorEnrollmentRequired = "second_factor_enrollment_required", 403;
    /// A sensitive command needs a password check within the last 10 minutes.
    ReauthenticationRequired = "reauthentication_required", 403;
    /// Missing, hidden and unknown resources and operations all answer with this code and the same body.
    NotFound = "not_found", 404;
    /// The resource is visible but the role lacks the capability.
    Forbidden = "forbidden", 403;
    /// API keys cannot call this operation.
    OperationNotGranted = "operation_not_granted", 403;
    /// An API key would hold a capability its owner lacks on the node, or one only people hold.
    GrantExceedsPrincipal = "grant_exceeds_principal", 403;
    /// A mutation without the request marker or from another origin.
    OriginRejected = "origin_rejected", 403;
    /// `expected_revision` is older than the resource. `current` carries the head.
    StaleRevision = "stale_revision", 409;
    /// The idempotency key was used for another operation or other arguments.
    IdempotencyConflict = "idempotency_conflict", 409;
    /// The node has descendants and the request did not confirm their number. `current`
    /// carries the number of descendant nodes.
    NodeNotEmpty = "node_not_empty", 409;
    /// The request does not parse: bad JSON, wrong types, missing idempotency key.
    MalformedRequest = "malformed_request", 400;
    /// Arguments parse but fail validation. `errors` names each field.
    InvalidArguments = "invalid_arguments", 422;
    /// The body or query string exceeds its byte limit.
    PayloadTooLarge = "payload_too_large", 413;
    RateLimited = "rate_limited", 429;
    /// A query ran past its 30 second budget and was cancelled.
    QueryTimeout = "query_timeout", 503;
    Internal = "internal", 500;
}

/// Why one field failed validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FieldErrorCode {
    Required,
    TooShort,
    TooLong,
    /// More items than the limit allows.
    TooMany,
    InvalidFormat,
    InvalidValue,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
pub struct FieldError {
    pub code: FieldErrorCode,
    /// The limit that was exceeded, for `too_short`, `too_long` and `too_many`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub limit: Option<u32>,
}

/// The body of every non-2xx API answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
pub struct ApiErrorBody {
    pub error_code: ErrorCode,
    /// English text for logs and developers. Not shown to people and not stable.
    pub message: String,
    /// Field errors keyed by argument name, for `invalid_arguments`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub errors: Option<BTreeMap<String, Vec<FieldError>>>,
    /// The current revision for `stale_revision`, the descendant count for `node_not_empty`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub current: Option<i64>,
}
