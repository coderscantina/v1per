//! An HTTP server that records every request. `GET` answers like a Discord webhook info call.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Router;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, Method, StatusCode, Uri};
use axum::response::IntoResponse;

#[derive(Clone, Debug)]
pub struct Captured {
    pub method: Method,
    /// Path and query.
    pub target: String,
    pub headers: HeaderMap,
    pub body: Bytes,
}

impl Captured {
    pub fn json(&self) -> serde_json::Value {
        serde_json::from_slice(&self.body).expect("JSON request body")
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name).and_then(|v| v.to_str().ok())
    }

    pub fn text(&self) -> String {
        String::from_utf8(self.body.to_vec()).expect("UTF-8 body")
    }
}

#[derive(Clone)]
pub struct Reply {
    /// Status for non-GET requests.
    pub status: u16,
    pub body: &'static str,
    /// `avatar` in the GET answer.
    pub avatar: Option<&'static str>,
    pub delay: Duration,
}

impl Default for Reply {
    fn default() -> Self {
        Self {
            status: 204,
            body: "",
            avatar: Some("a1b2c3"),
            delay: Duration::ZERO,
        }
    }
}

#[derive(Clone)]
struct Shared {
    reply: Reply,
    requests: Arc<Mutex<Vec<Captured>>>,
}

pub struct FakeHttp {
    pub url: String,
    requests: Arc<Mutex<Vec<Captured>>>,
}

impl FakeHttp {
    pub async fn start() -> Self {
        Self::with(Reply::default()).await
    }

    pub async fn with(reply: Reply) -> Self {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let shared = Shared {
            reply,
            requests: requests.clone(),
        };
        let app = Router::new().fallback(record).with_state(shared);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        Self { url, requests }
    }

    pub fn requests(&self) -> Vec<Captured> {
        self.requests.lock().unwrap().clone()
    }

    /// The only non-GET request.
    pub fn posted(&self) -> Captured {
        let mut posted: Vec<_> = self
            .requests()
            .into_iter()
            .filter(|r| r.method != Method::GET)
            .collect();
        assert_eq!(posted.len(), 1, "expected exactly one non-GET request");
        posted.remove(0)
    }
}

async fn record(
    State(shared): State<Shared>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    let is_get = method == Method::GET;
    shared.requests.lock().unwrap().push(Captured {
        method,
        target: uri
            .path_and_query()
            .map(ToString::to_string)
            .unwrap_or_default(),
        headers,
        body,
    });
    tokio::time::sleep(shared.reply.delay).await;
    if is_get {
        let info = serde_json::json!({ "id": "1", "avatar": shared.reply.avatar });
        return (StatusCode::OK, info.to_string()).into_response();
    }
    let status = StatusCode::from_u16(shared.reply.status).unwrap();
    (status, shared.reply.body).into_response()
}
