//! `GET /api/subscribe?topic=<topic>[@<seq>]&topic=...`: the events of up to 32 topics after
//! their snapshots' cursors, as server-sent events of [`TopicFrame`]s on one stream.
//!
//! Each topic opens with `subscribed`, catches up from its cursor in pages, then waits for the
//! hub's wake after each commit and reads on from its last cursor, so a missed wake loses
//! nothing. Topics with pending reads take turns, so one long catch-up does not hold up the
//! others. A cursor ahead of the head (a restored database) is `expired`. The session and each
//! topic are authorized again before every frame and every 5 seconds while idle; a topic whose
//! access is gone, or cannot be checked, ends with `resync` and reason `unavailable` while the
//! others go on. The stream ends when every topic has ended or the server stops. The older
//! `topic=<topic>&cursor=<topic>@<seq>` form still works: a `cursor` names a listed topic.
//!
//! Topics: `instance` needs any signed-in person, `user:<id>` that person, `node:<id>`,
//! `members:<id>`, `monitors:<id>`, `monitor-tree:<id>`, `probes:<id>`, `channels:<id>`,
//! `incidents:<id>`, `maintenance:<id>`, `components:<id>` and `pages:<id>` a scope on the
//! node (a person's or an API key's), and `page:<id>` one on the page's node. An
//! enrollment-only session follows only its own topic. A node the caller cannot read answers
//! like an unknown one, and one refused topic refuses the whole request; anything that does
//! not parse, a repeated topic or more than 32 topics is `malformed_request`. `EventSource`
//! sends no Origin on a same-origin request, so only a foreign Origin is refused; a cross-site
//! page cannot read the response anyway.

use std::collections::VecDeque;
use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::{Request, State};
use axum::http::{HeaderMap, header};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use tokio::sync::broadcast::{self, error::RecvError};
use tokio::sync::watch;
use tokio::time::Instant;
use v1per_protocol::envelope::Topic;
use v1per_protocol::errors::ErrorCode;
use v1per_protocol::ids::{NodeId, PageId, UserId};
use v1per_protocol::live::{LiveEvent, LiveFrame, ResyncReason, TopicFrame};

use super::client_address;
use super::operations::same_origin;
use crate::error::ApiError;
use crate::events;
use crate::identity::{self, Caller};
use crate::nodes::scope;
use crate::state::AppState;

/// Authorization is checked again at least this often while no frame is sent.
const RECHECK: Duration = Duration::from_secs(5);
/// Events per `events` frame; a longer catch-up sends several.
const PAGE: usize = 500;
/// Topics one stream may carry; a screen with more opens a second stream.
const MAX_TOPICS: usize = 32;

#[derive(Deserialize)]
struct Params {
    #[serde(default)]
    topic: Vec<String>,
    #[serde(default)]
    cursor: Vec<String>,
}

/// Who may follow a topic.
#[derive(Debug, PartialEq)]
enum Audience {
    AnyPerson,
    Person(UserId),
    Node(NodeId),
    /// A status page's own topic: a scope on the page's node.
    Page(PageId),
}

/// A requested topic, its audience and the cursor's sequence, if any.
type Wanted = (Topic, Audience, Option<i64>);

/// The requested topics. Parsed strictly: an unknown or repeated topic, or a cursor of a topic
/// not listed, is refused.
fn parse(params: &Params) -> Option<Vec<Wanted>> {
    if params.topic.is_empty() || params.topic.len() > MAX_TOPICS {
        return None;
    }
    let mut out: Vec<Wanted> = Vec::with_capacity(params.topic.len());
    for entry in &params.topic {
        let (name, seq) = match entry.rsplit_once('@') {
            Some((name, seq)) => (name, Some(sequence(seq)?)),
            None => (entry.as_str(), None),
        };
        let (topic, audience) = topic(name)?;
        if out.iter().any(|(t, ..)| t.as_str() == topic.as_str()) {
            return None;
        }
        out.push((topic, audience, seq));
    }
    for cursor in &params.cursor {
        let (name, seq) = cursor.rsplit_once('@')?;
        let slot = out.iter_mut().find(|(t, ..)| t.as_str() == name)?;
        if slot.2.replace(sequence(seq)?).is_some() {
            return None;
        }
    }
    Some(out)
}

fn sequence(seq: &str) -> Option<i64> {
    seq.parse::<i64>().ok().filter(|seq| *seq >= 0)
}

fn topic(name: &str) -> Option<(Topic, Audience)> {
    Some(match name.split_once(':') {
        None if name == "instance" => (Topic::instance(), Audience::AnyPerson),
        Some(("user", id)) if !id.is_empty() => {
            let user = UserId::new(id);
            (Topic::user(&user), Audience::Person(user))
        }
        Some((
            kind @ ("node" | "members" | "monitors" | "monitor-tree" | "probes" | "channels"
            | "incidents" | "maintenance" | "components" | "pages"),
            id,
        )) if id.bytes().all(|b| b.is_ascii_digit()) => {
            let node: NodeId = id.parse().ok()?;
            let topic = match kind {
                "node" => Topic::node(node),
                "members" => Topic::members(node),
                "monitor-tree" => Topic::monitor_tree(node),
                "probes" => Topic::probes(node),
                "channels" => Topic::channels(node),
                "incidents" => Topic::incidents(node),
                "maintenance" => Topic::maintenance(node),
                "components" => Topic::components(node),
                "pages" => Topic::pages(node),
                _ => Topic::monitors(node),
            };
            (topic, Audience::Node(node))
        }
        Some(("page", id)) if id.bytes().all(|b| b.is_ascii_digit()) => {
            let page: PageId = id.parse().ok()?;
            (Topic::page(page), Audience::Page(page))
        }
        _ => return None,
    })
}

pub async fn subscribe(State(state): State<AppState>, request: Request) -> Response {
    let (parts, _) = request.into_parts();
    let address = client_address(&parts);
    let headers = parts.headers;
    if let Some(origin) = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok())
        && !same_origin(&state.config, &headers, origin)
    {
        return ApiError::new(ErrorCode::OriginRejected).into_response();
    }
    let params: Option<Params> =
        serde_html_form::from_str(parts.uri.query().unwrap_or_default()).ok();
    let Some(requested) = params.as_ref().and_then(parse) else {
        return ApiError::with_message(
            ErrorCode::MalformedRequest,
            "expected 1 to 32 distinct topic=<topic> or topic=<topic>@<seq>",
        )
        .into_response();
    };
    let follows = match open(&state, &headers, &address, requested).await {
        Ok(follows) => follows,
        Err(error) => return error.into_response(),
    };

    let mut queue = VecDeque::new();
    for follow in &follows {
        queue.push_back(frame(
            &follow.topic,
            LiveFrame::Subscribed {
                head: follow.topic.cursor(follow.head),
                cursor: follow.topic.cursor(follow.sent),
            },
        ));
    }
    for follow in follows.iter().filter(|follow| follow.done) {
        queue.push_back(frame(
            &follow.topic,
            LiveFrame::Expired {
                head: follow.topic.cursor(follow.head),
            },
        ));
    }
    let (wake, closing) = state.hub.subscribe();
    let live = Live {
        state,
        headers,
        address,
        follows,
        turn: 0,
        wake,
        closing,
        next_check: Instant::now() + RECHECK,
        queue,
        stopping: false,
    };
    let stream = futures_util::stream::unfold(live, |mut live| async move {
        let frame = live.next().await?;
        let event = Event::default()
            .json_data(&frame)
            .expect("frames serialize");
        Some((Ok::<_, Infallible>(event), live))
    });
    Sse::new(stream)
        .keep_alive(KeepAlive::default())
        .into_response()
}

/// Authorizes every requested topic and reads its head.
async fn open(
    state: &AppState,
    headers: &HeaderMap,
    address: &str,
    requested: Vec<Wanted>,
) -> Result<Vec<Follow>, ApiError> {
    let caller = caller(state, headers, address).await?;
    let mut follows = Vec::with_capacity(requested.len());
    for (topic, audience, cursor) in requested {
        allowed(state, &caller, &audience).await?;
        let head = {
            let mut conn = state.db.reader.acquire().await?;
            events::head(&mut conn, &topic).await?
        };
        let sent = cursor.unwrap_or(head);
        follows.push(Follow {
            topic,
            audience,
            head,
            sent,
            unread: true,
            done: sent > head,
        });
    }
    Ok(follows)
}

/// A signed-in caller; anonymous callers are `unauthenticated`.
async fn caller(state: &AppState, headers: &HeaderMap, address: &str) -> Result<Caller, ApiError> {
    let caller =
        identity::resolve_caller(&state.db, headers, address.to_owned(), state.clock.now_ms())
            .await?;
    match caller {
        Caller::Anonymous { .. } => Err(ApiError::new(ErrorCode::Unauthenticated)),
        caller => Ok(caller),
    }
}

/// Whether `caller` may follow a topic of `audience`. Someone else's personal topic and an
/// unreadable node are `not_found`, as an unknown topic would be.
async fn allowed(state: &AppState, caller: &Caller, audience: &Audience) -> Result<(), ApiError> {
    match (audience, caller) {
        (Audience::Person(owner), _) if caller.user() == Some(owner) => Ok(()),
        (Audience::Person(_), _) | (Audience::AnyPerson, Caller::Key(_)) => {
            Err(ApiError::not_found())
        }
        (
            _,
            Caller::Person {
                enrollment_only: true,
                ..
            },
        ) => Err(ApiError::new(ErrorCode::SecondFactorEnrollmentRequired)),
        (Audience::AnyPerson, _) => Ok(()),
        (Audience::Node(node), _) => {
            let mut conn = state.db.reader.acquire().await?;
            scope::resolve(&mut conn, caller, *node).await.map(|_| ())
        }
        (Audience::Page(page), _) => {
            let mut conn = state.db.reader.acquire().await?;
            let node: i64 = sqlx::query_scalar("SELECT node_id FROM pages WHERE id = ?")
                .bind(page.get())
                .fetch_optional(&mut *conn)
                .await?
                .ok_or_else(ApiError::not_found)?;
            scope::resolve(&mut conn, caller, NodeId::new(node))
                .await
                .map(|_| ())
        }
    }
}

fn frame(topic: &Topic, frame: LiveFrame) -> TopicFrame {
    TopicFrame {
        topic: topic.as_str().to_owned(),
        frame,
    }
}

/// One topic of a stream.
struct Follow {
    topic: Topic,
    audience: Audience,
    /// The head when the stream opened.
    head: i64,
    /// The sequence of the last event a frame carried, or the cursor subscribed from.
    sent: i64,
    /// The topic may have events after `sent`.
    unread: bool,
    /// The topic sent `resync` or `expired`; nothing more goes out for it.
    done: bool,
}

enum Wake {
    Topic(Arc<str>),
    Lagged,
    Recheck,
    Stopping,
}

struct Live {
    state: AppState,
    headers: HeaderMap,
    address: String,
    follows: Vec<Follow>,
    /// Where the next read starts looking, so topics with pending reads take turns.
    turn: usize,
    wake: broadcast::Receiver<Arc<str>>,
    closing: watch::Receiver<bool>,
    next_check: Instant,
    queue: VecDeque<TopicFrame>,
    stopping: bool,
}

impl Live {
    async fn next(&mut self) -> Option<TopicFrame> {
        loop {
            if let Some(frame) = self.queue.pop_front() {
                return Some(frame);
            }
            if self.stopping || self.follows.iter().all(|follow| follow.done) {
                return None;
            }
            match self.take_unread() {
                Some(i) => {
                    if let Some(frame) = self.read(i).await {
                        return Some(frame);
                    }
                }
                None => self.wait().await,
            }
        }
    }

    fn take_unread(&mut self) -> Option<usize> {
        let count = self.follows.len();
        let i = (0..count)
            .map(|k| (self.turn + k) % count)
            .find(|&i| self.follows[i].unread && !self.follows[i].done)?;
        self.turn = i + 1;
        self.follows[i].unread = false;
        Some(i)
    }

    /// The next page of topic `i` as a frame, or nothing when it has no new events. Reads take
    /// a replay permit, so many reconnecting browsers share two readers. A failed read or a
    /// refused topic ends the topic with `resync`.
    async fn read(&mut self, i: usize) -> Option<TopicFrame> {
        let page = match self.page(i).await {
            Ok(page) => page,
            Err(error) => {
                let topic = self.follows[i].topic.as_str();
                tracing::warn!(%error, topic, "subscription read failed");
                return Some(self.end(i));
            }
        };
        let last = page.last()?.seq;
        // A full page may have more behind it.
        self.follows[i].unread = page.len() == PAGE;
        if !self.allowed(i).await {
            return Some(self.end(i));
        }
        let follow = &mut self.follows[i];
        follow.sent = last;
        Some(frame(
            &follow.topic,
            LiveFrame::Events {
                cursor: follow.topic.cursor(last),
                events: page,
            },
        ))
    }

    async fn page(&self, i: usize) -> Result<Vec<LiveEvent>, sqlx::Error> {
        let _permit = self
            .state
            .hub
            .replay
            .acquire()
            .await
            .expect("the replay semaphore is never closed");
        let follow = &self.follows[i];
        let mut conn = self.state.db.reader.acquire().await?;
        events::after(&mut conn, &follow.topic, follow.sent, PAGE).await
    }

    /// Waits until a topic may have new events or a re-check ended one.
    async fn wait(&mut self) {
        loop {
            let wake = tokio::select! {
                woke = self.wake.recv() => match woke {
                    Ok(topic) => Wake::Topic(topic),
                    Err(RecvError::Lagged(_)) => Wake::Lagged,
                    Err(RecvError::Closed) => Wake::Stopping,
                },
                _ = tokio::time::sleep_until(self.next_check) => Wake::Recheck,
                _ = self.closing.wait_for(|closing| *closing) => Wake::Stopping,
            };
            match wake {
                Wake::Topic(name) => {
                    let woken = self
                        .follows
                        .iter_mut()
                        .find(|follow| !follow.done && *follow.topic.as_str() == *name);
                    if let Some(follow) = woken {
                        follow.unread = true;
                        return;
                    }
                }
                // Missed wakes may include any topic's.
                Wake::Lagged => {
                    for follow in &mut self.follows {
                        follow.unread = true;
                    }
                    return;
                }
                Wake::Recheck => {
                    self.recheck().await;
                    if !self.queue.is_empty() {
                        return;
                    }
                }
                Wake::Stopping => {
                    self.stopping = true;
                    return;
                }
            }
        }
    }

    /// Checks the session and every open topic again; a refused topic ends with `resync`.
    async fn recheck(&mut self) {
        self.next_check = Instant::now() + RECHECK;
        let caller = caller(&self.state, &self.headers, &self.address).await.ok();
        for i in 0..self.follows.len() {
            if self.follows[i].done {
                continue;
            }
            let ok = match &caller {
                Some(caller) => allowed(&self.state, caller, &self.follows[i].audience)
                    .await
                    .is_ok(),
                None => false,
            };
            if !ok {
                let frame = self.end(i);
                self.queue.push_back(frame);
            }
        }
    }

    async fn allowed(&self, i: usize) -> bool {
        match caller(&self.state, &self.headers, &self.address).await {
            Ok(caller) => allowed(&self.state, &caller, &self.follows[i].audience)
                .await
                .is_ok(),
            Err(_) => false,
        }
    }

    /// Ends topic `i`: subscribe again from the last cursor handed out.
    fn end(&mut self, i: usize) -> TopicFrame {
        let follow = &mut self.follows[i];
        follow.done = true;
        frame(
            &follow.topic,
            LiveFrame::Resync {
                cursor: follow.topic.cursor(follow.sent),
                reason: ResyncReason::Unavailable,
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{Audience, Params, parse};
    use v1per_protocol::ids::{NodeId, UserId};

    fn params(topics: &[&str], cursors: &[&str]) -> Params {
        Params {
            topic: topics.iter().map(|t| (*t).into()).collect(),
            cursor: cursors.iter().map(|c| (*c).into()).collect(),
        }
    }

    #[test]
    fn cursors_belong_to_their_topic() {
        let seq = |p: Params| parse(&p).map(|topics| topics[0].2);
        assert_eq!(seq(params(&["instance"], &["instance@7"])), Some(Some(7)));
        assert_eq!(seq(params(&["instance@7"], &[])), Some(Some(7)));
        assert_eq!(seq(params(&["instance"], &[])), Some(None));
        assert_eq!(
            parse(&params(&["user:u1"], &[])).unwrap()[0].1,
            Audience::Person(UserId::new("u1"))
        );
        assert_eq!(
            parse(&params(&["node:7"], &["node:7@2"])).unwrap()[0].1,
            Audience::Node(NodeId::new(7))
        );
        for (topics, cursors) in [
            (&["instance"][..], &["user:u1@7"][..]),
            (&["instance"], &["instance@-1"]),
            (&["instance@3"], &["instance@4"]),
            (&["instance@x"], &[]),
            (&["node:n1"], &[]),
            (&["node:-1"], &[]),
            (&["user:"], &[]),
            (&[], &[]),
        ] {
            assert!(
                parse(&params(topics, cursors)).is_none(),
                "{topics:?} {cursors:?}"
            );
        }
    }

    #[test]
    fn a_stream_takes_up_to_32_distinct_topics() {
        let topics: Vec<String> = (1..=33).map(|n| format!("node:{n}@0")).collect();
        let names: Vec<&str> = topics.iter().map(String::as_str).collect();
        let parsed = parse(&params(&names[..32], &[])).unwrap();
        assert_eq!(parsed.len(), 32);
        assert!(parse(&params(&names, &[])).is_none());
        assert!(parse(&params(&["node:1", "node:1@4"], &[])).is_none());
        let both = parse(&params(&["instance@2", "node:1"], &["node:1@4"])).unwrap();
        assert_eq!((both[0].2, both[1].2), (Some(2), Some(4)));
    }
}
