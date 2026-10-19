//! Test harness: a real router over a temporary data directory with two accounts.

#![allow(dead_code)]

pub mod fakes;

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU16, AtomicUsize, Ordering};
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::http::{HeaderMap, Request, StatusCode, header};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tempfile::TempDir;
use tower::ServiceExt;
use v1per_protocol::envelope::Topic;
use v1per_protocol::ids::UserId;
use v1per_server::background::Background;
use v1per_server::clock::Clock;
use v1per_server::config::Config;
use v1per_server::db::{Db, now_ms};
use v1per_server::identity::secrets::SecretKey;
use v1per_server::identity::{self, SESSION_COOKIE};
use v1per_server::{AppState, events, router};
use v1per_storage::{Budgets, Storage};

pub const HOST: &str = "v1per.test";
pub const ORIGIN: &str = "http://v1per.test";
pub const PASSWORD: &str = "correct horse battery";

/// The argon2 hash of [`PASSWORD`], computed once per test binary.
fn password_hash() -> String {
    static HASH: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    HASH.get_or_init(|| identity::hash_password(PASSWORD))
        .clone()
}

pub struct Harness {
    pub state: AppState,
    router: Router,
    /// The instance admin, `admin@example.test`.
    pub admin: UserId,
    /// A person without instance rights, `member@example.test`.
    pub member: UserId,
    password_hash: String,
    _data: TempDir,
    _web: TempDir,
}

pub struct Answer {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub bytes: Vec<u8>,
}

impl Answer {
    pub fn json(&self) -> Value {
        serde_json::from_slice(&self.bytes).unwrap_or(Value::Null)
    }

    pub fn code(&self) -> String {
        self.json()["error_code"]
            .as_str()
            .unwrap_or_default()
            .to_owned()
    }

    pub fn header(&self, name: &str) -> &str {
        self.headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
    }
}

/// Small engine budgets and a short transfer delay, so heartbeats reach reads quickly.
fn budgets() -> Budgets {
    Budgets {
        engine_memory_bytes: 256 * 1024 * 1024,
        transfer_delay: Duration::from_millis(20),
        ..Budgets::default()
    }
}

/// A built web app with the admin and the status page entries.
fn web_dir() -> TempDir {
    let web = TempDir::new().unwrap();
    std::fs::create_dir_all(web.path().join("assets")).unwrap();
    std::fs::write(
        web.path().join("index.html"),
        "<!doctype html><div id=app></div>",
    )
    .unwrap();
    std::fs::write(web.path().join("assets/app-3f9a1c.js"), "console.log(1)").unwrap();
    std::fs::write(
        web.path().join("status.html"),
        "<!doctype html><html lang=\"en\"><head><title>Status</title></head><body><div id=status></div></body></html>",
    )
    .unwrap();
    web
}

/// The control database, heartbeat storage and state over `dir`.
async fn open(dir: &Path, web: &Path, clock: Clock) -> AppState {
    let db = Db::open(dir).await.unwrap();
    let storage = Arc::new(Storage::open(dir, budgets()).await.unwrap());
    let config = Config {
        data_dir: dir.to_path_buf(),
        bind: "127.0.0.1:0".parse().unwrap(),
        web_dir: Some(PathBuf::from(web)),
        public_origin: None,
        instance_name: Some("Test instance".into()),
    };
    let secrets = SecretKey::load_or_create(dir, None, None).unwrap();
    AppState::new(db, config, clock, secrets, storage)
}

impl Harness {
    pub async fn new() -> Self {
        Self::with_clock(Clock::system()).await
    }

    /// A harness on a clock that only [`Clock::advance`] moves, starting now.
    pub async fn manual() -> Self {
        Self::with_clock(Clock::manual(now_ms())).await
    }

    pub async fn with_clock(clock: Clock) -> Self {
        let data = TempDir::new().unwrap();
        let web = web_dir();
        let state = open(data.path(), web.path(), clock).await;

        let mut tx = state.db.writer.begin().await.unwrap();
        let hash = password_hash();
        let admin = identity::create_user(
            &mut tx,
            "admin@example.test",
            "Admin",
            &hash,
            true,
            now_ms(),
        )
        .await
        .unwrap();
        let member = identity::create_user(
            &mut tx,
            "member@example.test",
            "Member",
            &hash,
            false,
            now_ms(),
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();

        Self {
            router: router(state.clone()),
            state,
            admin,
            member,
            password_hash: hash,
            _data: data,
            _web: web,
        }
    }

    /// A harness over a data directory with `admin@example.test`, such as one `v1per init` or
    /// an import wrote, or a restored harness directory. The member is created unless it
    /// exists.
    pub async fn over(data: TempDir) -> Self {
        let web = web_dir();
        let state = open(data.path(), web.path(), Clock::system()).await;
        let id_of = |email: &'static str| {
            let reader = state.db.reader.clone();
            async move {
                sqlx::query_scalar::<_, String>("SELECT id FROM users WHERE email = ?")
                    .bind(email)
                    .fetch_optional(&reader)
                    .await
                    .unwrap()
            }
        };
        let admin = id_of("admin@example.test").await.unwrap();
        let hash = password_hash();
        let member = match id_of("member@example.test").await {
            Some(member) => UserId::new(member),
            None => {
                let mut conn = state.db.writer.acquire().await.unwrap();
                identity::create_user(
                    &mut conn,
                    "member@example.test",
                    "Member",
                    &hash,
                    false,
                    now_ms(),
                )
                .await
                .unwrap()
            }
        };
        Self {
            router: router(state.clone()),
            state,
            admin: UserId::new(admin),
            member,
            password_hash: hash,
            _data: data,
            _web: web,
        }
    }

    /// A session cookie for `user`, created directly so tests skip password hashing. It passed
    /// a password check just now, so sensitive commands go through.
    pub async fn session(&self, user: &UserId) -> String {
        let now = self.state.clock.now_ms();
        self.session_reauthenticated(user, Some(now)).await
    }

    /// A session cookie whose last password check was at `reauthenticated_at`, or never.
    pub async fn session_reauthenticated(
        &self,
        user: &UserId,
        reauthenticated_at: Option<i64>,
    ) -> String {
        let (token, digest) = identity::new_session_token();
        let now = self.state.clock.now_ms();
        sqlx::query(
            "INSERT INTO sessions (id, user_id, created_at, expires_at, last_seen_at, reauthenticated_at)
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(digest)
        .bind(user.as_str())
        .bind(now)
        // A day, so tests that move a manual clock keep their sessions.
        .bind(now + 86_400_000)
        .bind(now)
        .bind(reauthenticated_at)
        .execute(&self.state.db.writer)
        .await
        .unwrap();
        format!("{SESSION_COOKIE}={token}")
    }

    /// Another person without instance rights, `<alias>@example.test` with [`PASSWORD`].
    pub async fn person(&self, alias: &str) -> UserId {
        let mut conn = self.state.db.writer.acquire().await.unwrap();
        identity::create_user(
            &mut conn,
            &format!("{alias}@example.test"),
            alias,
            &self.password_hash,
            false,
            now_ms(),
        )
        .await
        .unwrap()
    }

    /// The root node's ID.
    pub async fn root(&self) -> String {
        let id: i64 = sqlx::query_scalar("SELECT id FROM nodes WHERE parent_id IS NULL")
            .fetch_one(&self.state.db.reader)
            .await
            .unwrap();
        id.to_string()
    }

    /// A command with a fresh idempotency key.
    pub async fn run(&self, cookie: Option<&str>, name: &str, mut body: Value) -> Answer {
        body["idempotency_key"] = json!(uuid::Uuid::new_v4().to_string());
        self.command(cookie, name, body).await
    }

    /// A command that must succeed, returning its answer.
    pub async fn ok(&self, cookie: &str, name: &str, body: Value) -> Value {
        let answer = self.run(Some(cookie), name, body).await;
        assert_eq!(answer.status, StatusCode::OK, "{name}: {}", answer.json());
        answer.json()
    }

    /// Creates a node under `parent` and returns its ID.
    pub async fn node(&self, cookie: &str, parent: &str, name: &str) -> String {
        let created = self
            .ok(
                cookie,
                "nodes.create",
                json!({ "parent": parent, "name": name }),
            )
            .await;
        created["node"]["id"].as_str().unwrap().to_owned()
    }

    /// Gives `user` `role` on `node`: the admin invites, the person accepts while signed in.
    pub async fn grant(&self, user: &UserId, node: &str, role: &str) {
        let admin = self.session(&self.admin).await;
        let invited = self
            .ok(
                &admin,
                "members.invite",
                json!({ "node": node, "role": role, "email": "invitee@example.test" }),
            )
            .await;
        let code = invited["secret"]["code"].as_str().unwrap();
        let person = self.session(user).await;
        self.ok(&person, "invitations.accept", json!({ "code": code }))
            .await;
    }

    /// A query sent with an `Authorization` header instead of a cookie.
    pub async fn query_with(&self, authorization: &str, name: &str, query: &str) -> Answer {
        let request = Request::get(format!("/api/queries/{name}?{query}"))
            .header(header::HOST, HOST)
            .header(header::AUTHORIZATION, authorization);
        self.send(request.body(Body::empty()).unwrap()).await
    }

    /// A command sent with an `Authorization` header, as a script would: no Origin, no marker.
    pub async fn command_with(&self, authorization: &str, name: &str, mut body: Value) -> Answer {
        body["idempotency_key"] = json!(uuid::Uuid::new_v4().to_string());
        let request = Request::post(format!("/api/commands/{name}"))
            .header(header::HOST, HOST)
            .header(header::AUTHORIZATION, authorization)
            .header(header::CONTENT_TYPE, "application/json");
        self.send(request.body(Body::from(body.to_string())).unwrap())
            .await
    }

    /// Records an event as a command would, and wakes subscribers after it is committed.
    pub async fn record(&self, topic: &Topic, kind: &str) -> i64 {
        let mut conn = self.state.db.writer.acquire().await.unwrap();
        let seq = events::record(&mut conn, topic, kind, &json!({}), now_ms())
            .await
            .unwrap();
        self.state.hub.publish(topic);
        seq
    }

    pub async fn send(&self, request: Request<Body>) -> Answer {
        let response = self.router.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let headers = response.headers().clone();
        let bytes = response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .to_vec();
        Answer {
            status,
            headers,
            bytes,
        }
    }

    pub async fn get(&self, path: &str) -> Answer {
        self.send(
            Request::get(path)
                .header(header::HOST, HOST)
                .body(Body::empty())
                .unwrap(),
        )
        .await
    }

    pub async fn query(&self, cookie: Option<&str>, name: &str, query: &str) -> Answer {
        let mut request =
            Request::get(format!("/api/queries/{name}?{query}")).header(header::HOST, HOST);
        if let Some(cookie) = cookie {
            request = request.header(header::COOKIE, cookie);
        }
        self.send(request.body(Body::empty()).unwrap()).await
    }

    /// A command as the web client sends it: same origin, request marker, JSON body.
    pub async fn command(&self, cookie: Option<&str>, name: &str, body: Value) -> Answer {
        let mut request = Request::post(format!("/api/commands/{name}"))
            .header(header::HOST, HOST)
            .header(header::ORIGIN, ORIGIN)
            .header("x-v1per-request", "1")
            .header(header::CONTENT_TYPE, "application/json");
        if let Some(cookie) = cookie {
            request = request.header(header::COOKIE, cookie);
        }
        self.send(request.body(Body::from(body.to_string())).unwrap())
            .await
    }

    pub async fn login(&self, key: &str, email: &str, password: &str) -> Answer {
        self.command(
            None,
            "auth.login",
            json!({ "idempotency_key": key, "email": email, "password": password }),
        )
        .await
    }

    /// Opens `GET /api/subscribe` with `query`. The body streams server-sent events.
    pub async fn subscribe(&self, cookie: Option<&str>, query: &str) -> axum::response::Response {
        let mut request =
            Request::get(format!("/api/subscribe?{query}")).header(header::HOST, HOST);
        if let Some(cookie) = cookie {
            request = request.header(header::COOKIE, cookie);
        }
        self.router
            .clone()
            .oneshot(request.body(Body::empty()).unwrap())
            .await
            .unwrap()
    }

    pub fn router(&self) -> Router {
        self.router.clone()
    }

    pub fn data_dir(&self) -> &Path {
        self._data.path()
    }

    /// Starts the built-in probe and the evaluator.
    pub fn start(&self) -> Background {
        Background::start(&self.state)
    }

    /// Stops `background` and closes storage and the control database as `serve` does, then
    /// opens everything again on the same data directory and clock, as a restarted server.
    pub async fn restart(self, background: Background) -> Self {
        background.stop().await;
        let Self {
            state,
            router: old_router,
            admin,
            member,
            password_hash,
            _data,
            _web,
        } = self;
        let clock = state.clock.clone();
        state.storage.shutdown().await.unwrap();
        state.db.close().await;
        // The engines' files open again only after the old handles are gone.
        drop((state, old_router));
        let state = open(_data.path(), _web.path(), clock).await;
        Self {
            router: router(state.clone()),
            state,
            admin,
            member,
            password_hash,
            _data,
            _web,
        }
    }
}

/// Reads JSON frames from a server-sent event stream, keeping any partial or extra frame.
pub struct Frames {
    body: Body,
    text: String,
}

impl Frames {
    pub fn new(response: axum::response::Response) -> Self {
        Self {
            body: response.into_body(),
            text: String::new(),
        }
    }

    /// The next `count` frames, each within 10 seconds.
    pub async fn next(&mut self, count: usize) -> Vec<Value> {
        let mut out = Vec::new();
        loop {
            while out.len() < count
                && let Some(end) = self.text.find("\n\n")
            {
                let event: String = self.text.drain(..end + 2).collect();
                if let Some(data) = event.lines().find_map(|line| line.strip_prefix("data: ")) {
                    out.push(serde_json::from_str(data).unwrap());
                }
            }
            if out.len() == count {
                return out;
            }
            let frame = tokio::time::timeout(std::time::Duration::from_secs(10), self.body.frame())
                .await
                .expect("a frame in time")
                .expect("the stream stays open")
                .unwrap();
            if let Ok(data) = frame.into_data() {
                self.text.push_str(&String::from_utf8_lossy(&data));
            }
        }
    }

    /// Whether the stream ends within 10 seconds, skipping keep-alive comments.
    pub async fn ends(&mut self) -> bool {
        loop {
            match tokio::time::timeout(std::time::Duration::from_secs(10), self.body.frame()).await
            {
                Ok(None) => return true,
                Ok(Some(Ok(frame))) => {
                    let data = frame.into_data().unwrap_or_default();
                    if String::from_utf8_lossy(&data).contains("data: ") {
                        return false;
                    }
                }
                Ok(Some(Err(_))) | Err(_) => return false,
            }
        }
    }
}

/// A local HTTP target answering every request with `status`.
pub struct Target {
    pub url: String,
    status: Arc<AtomicU16>,
    hits: Arc<AtomicUsize>,
}

impl Target {
    pub async fn start() -> Self {
        let status = Arc::new(AtomicU16::new(200));
        let hits = Arc::new(AtomicUsize::new(0));
        let (answer, count) = (status.clone(), hits.clone());
        let app = Router::new().fallback(move || {
            count.fetch_add(1, Ordering::SeqCst);
            let code = StatusCode::from_u16(answer.load(Ordering::SeqCst)).unwrap();
            async move { (code, "target body") }
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address: SocketAddr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await });
        Self {
            url: format!("http://{address}/health"),
            status,
            hits,
        }
    }

    pub fn answer(&self, status: u16) {
        self.status.store(status, Ordering::SeqCst);
    }

    pub fn hits(&self) -> usize {
        self.hits.load(Ordering::SeqCst)
    }
}
