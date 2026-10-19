//! What a command or query handler receives besides its arguments.
//!
//! The dispatcher has already admitted the caller (credential, API key reach, enrollment-only
//! sessions). Handlers apply the rest in this order: resolve the node scope (`not_found`),
//! require the capability (`forbidden`), rank checks (`forbidden`), then
//! [`CommandCtx::require_fresh_login`] for sensitive commands (`reauthentication_required`),
//! then [`CommandCtx::check_revision`] (`stale_revision`). So a refusal never says more than
//! the caller may know.

use sqlx::{Sqlite, SqliteConnection, Transaction};
use v1per_protocol::access::Capability;
use v1per_protocol::envelope::Topic;
use v1per_protocol::ids::{NodeId, UserId};
use v1per_protocol::registry::Handler;

use crate::error::ApiError;
use crate::events;
use crate::identity::Caller;
use crate::nodes::scope::{self, NodeScope};
use crate::state::AppState;

impl Handler for AppState {
    type CommandCtx = CommandCtx;
    type QueryCtx = QueryCtx;
    type Error = ApiError;
}

/// A change to the caller's cookies, applied to the response after commit.
pub enum SessionChange {
    /// Sets the session cookie and clears any login challenge.
    Start(String),
    End,
    /// Sets the login challenge cookie between password and second factor.
    Challenge(String),
}

/// One command: its write transaction, the caller and what it did.
pub struct CommandCtx {
    tx: Transaction<'static, Sqlite>,
    caller: Caller,
    now: i64,
    last_event: Option<i64>,
    touched: Vec<Topic>,
    session: Option<SessionChange>,
    fresh_login: bool,
}

impl CommandCtx {
    pub fn new(tx: Transaction<'static, Sqlite>, caller: Caller, now: i64) -> Self {
        Self {
            tx,
            caller,
            now,
            last_event: None,
            touched: Vec::new(),
            session: None,
            fresh_login: false,
        }
    }

    pub fn conn(&mut self) -> &mut SqliteConnection {
        &mut self.tx
    }

    pub fn caller(&self) -> &Caller {
        &self.caller
    }

    pub fn now(&self) -> i64 {
        self.now
    }

    /// Resolves the caller's scope on `node` and requires `capability`. A node the caller
    /// cannot read is `not_found`, a missing capability `forbidden`.
    pub async fn node_scope(
        &mut self,
        node: NodeId,
        capability: Capability,
    ) -> Result<NodeScope, ApiError> {
        let scope = scope::resolve(&mut self.tx, &self.caller, node).await?;
        scope.require(capability)?;
        Ok(scope)
    }

    /// `forbidden` unless the caller is a signed-in instance admin, read now so a demotion
    /// applies at once.
    pub async fn require_instance_admin(&mut self) -> Result<UserId, ApiError> {
        scope::require_instance_admin(&mut self.tx, &self.caller).await
    }

    /// `reauthentication_required` unless this login session passed a password check within
    /// the last 10 minutes. Operations declared `reauth` must call it; the dispatcher refuses
    /// to commit otherwise.
    pub fn require_fresh_login(&mut self) -> Result<(), ApiError> {
        self.caller.require_fresh_login(self.now)?;
        self.fresh_login = true;
        Ok(())
    }

    /// `stale_revision` with the current head when `expected` differs from `topic`'s head.
    pub async fn check_revision(
        &mut self,
        topic: &Topic,
        expected: Option<i64>,
    ) -> Result<(), ApiError> {
        if let Some(expected) = expected {
            let head = events::head(&mut self.tx, topic).await?;
            if expected != head {
                return Err(ApiError::stale_revision(head));
            }
        }
        Ok(())
    }

    /// Records an event in this command's transaction. The last one becomes the revision, and
    /// subscribers of `topic` are woken after commit.
    pub async fn record(
        &mut self,
        topic: &Topic,
        kind: &str,
        payload: serde_json::Value,
    ) -> Result<i64, ApiError> {
        let seq = events::record(&mut self.tx, topic, kind, &payload, self.now).await?;
        self.last_event = Some(seq);
        if !self.touched.contains(topic) {
            self.touched.push(topic.clone());
        }
        Ok(seq)
    }

    /// Records an event that is a side effect of the command, such as on the topic of a status
    /// page showing what changed. Subscribers are woken after commit; the revision stays
    /// with the command's own topic.
    pub async fn record_also(
        &mut self,
        topic: &Topic,
        kind: &str,
        payload: serde_json::Value,
    ) -> Result<(), ApiError> {
        events::record(&mut self.tx, topic, kind, &payload, self.now).await?;
        if !self.touched.contains(topic) {
            self.touched.push(topic.clone());
        }
        Ok(())
    }

    pub fn start_session(&mut self, token: String) {
        self.session = Some(SessionChange::Start(token));
    }

    pub fn end_session(&mut self) {
        self.session = Some(SessionChange::End);
    }

    pub fn start_challenge(&mut self, token: String) {
        self.session = Some(SessionChange::Challenge(token));
    }

    pub(crate) fn into_parts(self) -> CommandParts {
        CommandParts {
            tx: self.tx,
            last_event: self.last_event,
            touched: self.touched,
            session: self.session,
            fresh_login: self.fresh_login,
        }
    }
}

/// What the dispatcher needs after the handler ran.
pub(crate) struct CommandParts {
    pub tx: Transaction<'static, Sqlite>,
    pub last_event: Option<i64>,
    pub touched: Vec<Topic>,
    pub session: Option<SessionChange>,
    /// The handler passed [`CommandCtx::require_fresh_login`].
    pub fresh_login: bool,
}

/// One query: a read transaction, so the snapshot and its topic head agree.
pub struct QueryCtx {
    tx: Transaction<'static, Sqlite>,
    caller: Caller,
}

impl QueryCtx {
    pub fn new(tx: Transaction<'static, Sqlite>, caller: Caller) -> Self {
        Self { tx, caller }
    }

    pub fn conn(&mut self) -> &mut SqliteConnection {
        &mut self.tx
    }

    pub fn caller(&self) -> &Caller {
        &self.caller
    }

    /// The query counterpart of [`CommandCtx::node_scope`].
    pub async fn node_scope(
        &mut self,
        node: NodeId,
        capability: Capability,
    ) -> Result<NodeScope, ApiError> {
        let scope = scope::resolve(&mut self.tx, &self.caller, node).await?;
        scope.require(capability)?;
        Ok(scope)
    }

    pub async fn require_instance_admin(&mut self) -> Result<UserId, ApiError> {
        scope::require_instance_admin(&mut self.tx, &self.caller).await
    }

    pub(crate) fn into_tx(self) -> Transaction<'static, Sqlite> {
        self.tx
    }
}
