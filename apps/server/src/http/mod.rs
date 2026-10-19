//! The router: admin API, Kuma's push URL and the probe agent protocol under `/api`, health,
//! runtime config, the web app, and status pages at `/s/{slug}` and on claimed hostnames.

pub(crate) mod operations;
mod subscribe;
mod web;

use axum::http::{HeaderName, HeaderValue, header};
use axum::response::IntoResponse;
use axum::routing::{any, get, post};
use axum::{Json, Router};
use serde_json::json;
use tower::ServiceBuilder;
use tower_http::compression::CompressionLayer;
use tower_http::set_header::SetResponseHeaderLayer;
use tower_http::trace::TraceLayer;

use crate::error::ApiError;
use crate::state::AppState;

async fn health() -> impl IntoResponse {
    Json(json!({ "status": "ok", "version": env!("CARGO_PKG_VERSION") }))
}

/// Unknown API paths and methods get the typed `not_found` body, never the web app.
async fn api_not_found() -> ApiError {
    ApiError::not_found()
}

pub fn router(state: AppState) -> Router {
    let api = Router::new()
        .route("/commands/{name}", post(operations::command))
        .route("/queries/{name}", get(operations::query))
        .route("/subscribe", get(subscribe::subscribe))
        .route("/push/{token}", any(crate::monitors::push))
        .route("/probe/assignments", post(crate::probes::assignments))
        .route("/probe/results", post(crate::probes::results))
        .route("/pages/{page}/logo", post(crate::pages::upload_logo))
        .fallback(api_not_found)
        .method_not_allowed_fallback(api_not_found)
        .with_state(state.clone());

    let web_dir = state.config.web_dir.clone();
    let hostnames = crate::pages::hostname_router(state.clone());
    Router::new()
        .route("/health", get(health))
        .nest("/api", api)
        .merge(crate::pages::instance_routes().with_state(state.clone()))
        .merge(web::router(state.clone(), web_dir.as_deref()))
        .layer(axum::middleware::from_fn_with_state(
            (state, hostnames),
            crate::pages::host_routing,
        ))
        .layer(
            ServiceBuilder::new()
                .layer(TraceLayer::new_for_http())
                .layer(CompressionLayer::new())
                .layer(SetResponseHeaderLayer::if_not_present(
                    header::X_CONTENT_TYPE_OPTIONS,
                    HeaderValue::from_static("nosniff"),
                ))
                .layer(SetResponseHeaderLayer::if_not_present(
                    header::X_FRAME_OPTIONS,
                    HeaderValue::from_static("DENY"),
                ))
                .layer(SetResponseHeaderLayer::if_not_present(
                    HeaderName::from_static("referrer-policy"),
                    HeaderValue::from_static("same-origin"),
                )),
        )
}

/// The client address of a request: the connection's peer.
pub(crate) fn client_address(parts: &axum::http::request::Parts) -> String {
    parts
        .extensions
        .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
        .map(|info| info.0.ip().to_string())
        .unwrap_or_else(|| "unknown".into())
}
