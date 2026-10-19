//! `/api/probe/*`, the agent protocol (`docs/contracts/probe-protocol.md`). Agents connect
//! out to the server; the server never dials them. Every request names its protocol version in
//! `x-v1per-probe-protocol` and authenticates with `Authorization: Bearer <token>`.

use std::time::Duration;

use axum::Json;
use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::request::Parts;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::de::DeserializeOwned;
use sha2::{Digest, Sha256};
use sqlx::SqliteConnection;
use tokio::sync::broadcast::error::RecvError;
use v1per_protocol::ids::ProbeId;
use v1per_protocol::limits::API_KEY_SCHEME;
use v1per_protocol::operations::monitors::HeartbeatStatus;

use super::wire::{
    self, AssignmentList, AssignmentsRequest, BATCH_ID_MAX_CHARS, BATCH_RESULTS_MAX, BatchAck,
    PROTOCOL, PROTOCOL_HEADER, Refusal, ResultBatch,
};
use crate::http::client_address;
use crate::identity::token_digest;
use crate::monitors::{self, Assignment, BatchRefusal};
use crate::state::AppState;

/// How long an assignment poll waits for a change: below common proxy timeouts.
const LONG_POLL: Duration = Duration::from_secs(25);
const ASSIGNMENTS_BODY_MAX: usize = 16 * 1024;
const RESULTS_BODY_MAX: usize = 4 * 1024 * 1024;

/// A refusal, answered with the protocol's error body.
pub(crate) struct Refused {
    status: StatusCode,
    error: &'static str,
    message: String,
}

impl IntoResponse for Refused {
    fn into_response(self) -> Response {
        let body = Refusal {
            error: self.error.to_owned(),
            message: self.message,
        };
        (self.status, Json(body)).into_response()
    }
}

fn refuse(status: StatusCode, error: &'static str, message: impl Into<String>) -> Refused {
    Refused {
        status,
        error,
        message: message.into(),
    }
}

fn unavailable() -> Refused {
    refuse(
        StatusCode::SERVICE_UNAVAILABLE,
        "unavailable",
        "The server cannot take this now. Try again.",
    )
}

fn malformed(message: impl Into<String>) -> Refused {
    refuse(StatusCode::BAD_REQUEST, "malformed_request", message)
}

/// The probe behind a request of a supported protocol version, or the refusal to send.
async fn admit(state: &AppState, parts: &Parts) -> Result<ProbeId, Refused> {
    let protocol = parts
        .headers
        .get(PROTOCOL_HEADER)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u32>().ok());
    if protocol != Some(PROTOCOL) {
        let agent = protocol.map_or_else(|| "none".to_owned(), |p| p.to_string());
        return Err(refuse(
            StatusCode::UPGRADE_REQUIRED,
            "protocol_unsupported",
            format!(
                "This server (v1per {}) speaks probe protocol {PROTOCOL}, the agent speaks {agent}. \
                 Run the agent from the same v1per release as the server.",
                env!("CARGO_PKG_VERSION")
            ),
        ));
    }
    let address = client_address(parts);
    if state.probe_misses.exhausted(&address) {
        return Err(refuse(
            StatusCode::TOO_MANY_REQUESTS,
            "rate_limited",
            "Too many unknown tokens from this address. Try again in a minute.",
        ));
    }
    let probe: Option<i64> = match bearer(&parts.headers) {
        Some(token) => {
            sqlx::query_scalar("SELECT id FROM probes WHERE token_hash = ? AND revoked_at IS NULL")
                .bind(token_digest(token))
                .fetch_optional(&state.db.reader)
                .await
                .map_err(|error| {
                    tracing::error!(%error, "reading probe tokens failed");
                    unavailable()
                })?
        }
        None => None,
    };
    probe.map(ProbeId::new).ok_or_else(|| {
        let _ = state.probe_misses.check(&address);
        unauthenticated()
    })
}

fn unauthenticated() -> Refused {
    refuse(
        StatusCode::UNAUTHORIZED,
        "unauthenticated",
        "The probe token is unknown or was revoked. Register the probe again for a new token.",
    )
}

fn bearer(headers: &HeaderMap) -> Option<&str> {
    let value = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let (scheme, token) = value.split_once(' ')?;
    (scheme == API_KEY_SCHEME && !token.is_empty()).then_some(token)
}

async fn parse<T: DeserializeOwned>(body: Body, limit: usize) -> Result<T, Refused> {
    let bytes = axum::body::to_bytes(body, limit).await.map_err(|_| {
        refuse(
            StatusCode::PAYLOAD_TOO_LARGE,
            "payload_too_large",
            format!("Request bodies are at most {limit} bytes."),
        )
    })?;
    serde_json::from_slice(&bytes).map_err(|error| malformed(error.to_string()))
}

/// `POST /api/probe/assignments`: records the agent's health and answers its assignment list
/// once it differs from the version the agent runs, waiting up to [`LONG_POLL`] for a change.
pub(crate) async fn assignments(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<AssignmentList>, Refused> {
    let (parts, body) = request.into_parts();
    let probe = admit(&state, &parts).await?;
    let request: AssignmentsRequest = parse(body, ASSIGNMENTS_BODY_MAX).await?;
    match long_poll(&state, probe, request).await {
        Ok(Some(list)) => Ok(Json(list)),
        Ok(None) => Err(unauthenticated()),
        Err(error) => {
            tracing::error!(%error, %probe, "answering a probe's assignment poll failed");
            Err(unavailable())
        }
    }
}

/// The list to answer, or `None` when the probe was revoked meanwhile.
async fn long_poll(
    state: &AppState,
    probe: ProbeId,
    request: AssignmentsRequest,
) -> anyhow::Result<Option<AssignmentList>> {
    let deadline = tokio::time::Instant::now() + LONG_POLL;
    // Subscribed before the first read, so a change committed after it wakes this poll.
    let (mut wake, mut closing) = state.hub.subscribe();
    let mut tx = state.db.writer.begin_with("BEGIN IMMEDIATE").await?;
    let online = super::touch(
        &mut tx,
        probe,
        state.clock.now_ms(),
        request.sent_at,
        Some(&request.health),
    )
    .await?;
    tx.commit().await?;
    if let Some(topic) = online {
        state.hub.publish(&topic);
    }

    let mut seen = None;
    let mut version = String::new();
    loop {
        let mut conn = state.db.reader.acquire().await?;
        let current = monitors::assignment_version(&mut conn).await?;
        if seen != Some(current) {
            seen = Some(current);
            let revoked: Option<i64> =
                sqlx::query_scalar("SELECT revoked_at FROM probes WHERE id = ?")
                    .bind(probe.get())
                    .fetch_one(&mut *conn)
                    .await?;
            if revoked.is_some() {
                return Ok(None);
            }
            let assignments = list(state, &mut conn, probe).await?;
            version = digest(&assignments)?;
            if request.version.as_deref() != Some(version.as_str()) {
                return Ok(Some(AssignmentList {
                    version,
                    assignments: Some(assignments),
                }));
            }
        }
        drop(conn);
        tokio::select! {
            woken = wake.recv() => if matches!(woken, Err(RecvError::Closed)) { break },
            _ = tokio::time::sleep_until(deadline) => break,
            _ = closing.wait_for(|closing| *closing) => break,
        }
    }
    Ok(Some(AssignmentList {
        version,
        assignments: None,
    }))
}

/// Every check `probe` runs, with unsealed credentials.
async fn list(
    state: &AppState,
    conn: &mut SqliteConnection,
    probe: ProbeId,
) -> anyhow::Result<Vec<wire::Assignment>> {
    let mut assignments = Vec::new();
    for active in monitors::active(conn, probe, false).await? {
        if let Some(Assignment::Check(check)) =
            monitors::assignment(conn, &state.secrets, active.monitor).await?
        {
            assignments.push(*check);
        }
    }
    Ok(assignments)
}

/// A list's version: the first 128 bits of the SHA-256 of its JSON, so an unchanged list keeps
/// its version across server restarts.
fn digest(assignments: &[wire::Assignment]) -> serde_json::Result<String> {
    let hash = Sha256::digest(serde_json::to_vec(assignments)?);
    Ok(hash[..16]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

/// `POST /api/probe/results`: admits a batch durably and idempotently, then answers.
pub(crate) async fn results(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<BatchAck>, Refused> {
    let (parts, body) = request.into_parts();
    let probe = admit(&state, &parts).await?;
    let batch: ResultBatch = parse(body, RESULTS_BODY_MAX).await?;
    if batch.batch.is_empty() || batch.batch.chars().count() > BATCH_ID_MAX_CHARS {
        return Err(malformed("`batch` must be 1 to 128 characters."));
    }
    if batch.results.len() > BATCH_RESULTS_MAX {
        return Err(malformed("A batch carries at most 1000 results."));
    }
    if batch
        .results
        .iter()
        .any(|result| result.status == HeartbeatStatus::Maintenance)
    {
        return Err(malformed("A probe reports down, up or pending."));
    }
    let id = batch.batch.clone();
    match state
        .evaluator
        .batch(probe, batch.batch, batch.sent_at, batch.results)
        .await
    {
        Ok(repeated) => Ok(Json(BatchAck {
            batch: id,
            repeated,
        })),
        Err(BatchRefusal::Conflict) => Err(refuse(
            StatusCode::CONFLICT,
            "batch_conflict",
            "This batch ID was admitted before with other results.",
        )),
        Err(BatchRefusal::Unavailable) => Err(unavailable()),
    }
}
