# Architecture patterns

v1per follows several patterns proven in holocron, the analytics app it is adapted from. This page describes each pattern in prose so it can be built from scratch without access to holocron. Type sketches are illustrative, not copied code, and names are v1per's to choose.

Other docs link to the sections below.

## Contents

- [Operation registry and generated contracts](#operation-registry-and-generated-contracts)
- [Schema export and drift check](#schema-export-and-drift-check)
- [Named commands, transactions, revisions and idempotency](#named-commands-transactions-revisions-and-idempotency)
- [Snapshot queries](#snapshot-queries)
- [Explicit authorized scope](#explicit-authorized-scope)
- [Control database boundary](#control-database-boundary)
- [Topics, cursors and subscriptions](#topics-cursors-and-subscriptions)
- [Typed client](#typed-client)
- [Live snapshot adapter](#live-snapshot-adapter)

## Operation registry and generated contracts

The browser talks to the server through named operations, not free-form routes. Two registries list them: one for commands (writes) and one for queries (reads). Each entry holds four things: the operation name, an access level (anonymous or signed-in person), the argument type, and the result type.

The registry is the single source of truth for three consumers:

- Dispatch. One HTTP handler per kind (`POST /api/commands/<name>`, `GET /api/queries/<name>`) looks the name up, checks access, parses the arguments into the declared type, and calls the matching function. An unknown name answers the same "not available" error as a resource the caller may not see.
- Generated TypeScript. The registry also yields the typed maps the web client uses (see below).
- Error codes. One closed enum of stable error codes belongs to the same protocol crate and is exported with it.

Each domain (nodes, monitors, status pages, incidents, imports, and so on) declares its own entries in its own file. A small macro or build step merges the domain lists. Two people working on two domains touch two files and nothing shared.

An illustrative entry:

```rust
// name (access): Args => Result;
create_monitor (Person): CreateMonitorArgs => CreateMonitorResult;
```

Wire types derive their TypeScript form from the Rust definition, so the Rust type is the only place a field is declared. Wire conventions are fixed once: snake case arguments, timestamps as Unix milliseconds, IDs as strings.

## Schema export and drift check

A single command (for example `export-types`) writes the generated files into the web app:

- One TypeScript file per wire type.
- `commands.ts` and `queries.ts`, each an interface keyed by operation name, with `args` and `result` (or `snapshot`) per name, plus a const list of the names.
- `errors.ts`, a const list of every error code.

Output is sorted by name, so regenerating an unchanged registry changes no byte. Every generated file starts with a marker comment saying it is generated and must not be edited.

The drift check is plain: run the export into a temporary directory in CI and diff it against the committed generated files. Any difference fails the build. A renamed operation or a changed argument then shows up as a compile error in the web app, not as a runtime failure.

```ts
export interface Commands {
  create_monitor: { args: CreateMonitorArgs; result: CreateMonitorResult }
}
export const commandNames = ['create_monitor'] as const satisfies readonly (keyof Commands)[]
```

## Named commands, transactions, revisions and idempotency

A command is `POST /api/commands/<name>` with a JSON body of the idempotency key plus the arguments:

```ts
type CommandBody<A> = { idempotency_key: string } & A
type CommandAnswer<R> = { revision: number } & R
```

The handler follows a fixed sequence:

1. Look up the name and access level. Rate-limit commands that take a password or token guess, per address.
2. Resolve the caller from the session. Anonymous commands use the client address as the key owner.
3. Split the body into the key and the arguments. The key must be present, non-empty and at most 128 characters.
4. Open one write transaction that takes the write lock up front (`BEGIN IMMEDIATE` in SQLite).
5. Look up the key for this owner. If a stored response exists and is younger than 24 hours, return it without running anything. If it was stored for a different command name, refuse with a key-reused error.
6. Run the command function inside the same transaction. It receives the transaction, a per-command context, and the parsed arguments.
7. Compute the revision (below), store the response under the key and owner in the same transaction, and commit.
8. After commit only: notify subscribers of every topic the command touched, wake background tasks that read what the command wrote, and run deferred cleanup.

Two rules follow from this order. A response is stored only if the effect committed, so a retry after a crash either replays the stored answer or runs the command fresh, never half and half. Background work and notifications happen after commit, so nothing reads the state from before the command.

Results that hold one-time secrets (a new API key, a status page share token, a probe agent token) or data an erasure must be able to remove are stripped before the response is stored. A replay then returns a result without the secret. The first, live answer still has it.

Revisions: every topic has a monotonic sequence of state events. A command that changes state records an event describing the change in its own transaction. The answer's `revision` is the sequence of the last event it recorded, or the head of a default topic when it recorded none.

Commands that change a scoped resource accept an optional `expected_revision`. After the scope resolves, the command compares it to the topic head and refuses with a 409 stale-revision error that carries the current head. The check runs after scope resolution so a stale revision never confirms that a resource exists to someone who cannot see it.

Probe result batches follow the same idea with a batch ID as the key: the server stores the batch in the inbox and acknowledges it in one SQLite transaction, and a resent batch is acknowledged again without being stored twice.

The context object offers: the authenticated caller, a method to resolve scope with an optional expected revision, a method to record an event on a topic, and a list of tasks to wake after commit.

## Snapshot queries

A query is `GET /api/queries/<name>?...` and answers a snapshot:

```ts
type Snapshot<S> = { revision: number; cursor: string } & S
```

The snapshot and the head of its topic are read in one read transaction. The `cursor` therefore marks exactly where the snapshot ends, and subscribing from it neither misses nor repeats an event.

Examples are the monitor list of a node, the open incidents of a node, and one status page with its components.

Query arguments are query-string values: arrays repeat the key, absent values are left out. Pagination uses a default page size (25) and a hard maximum (100).

A query handler returns its snapshot together with the topic its cursor belongs to. The handler for the query kind builds the revision and cursor from that topic.

## Explicit authorized scope

Domain operations never receive a bare resource ID plus a caller. They receive a scope value, a proof that authorization already happened for this caller on this resource.

Rules for the scope type:

- Its fields are private and only the authorization module can construct it. Domain code cannot forge one.
- One resolve function turns (principal, node ID) into a scope or a refusal. It checks membership and role, and returns the set of capabilities the role grants.
- Every refusal is the same "not available" value. An unknown resource, a deleted one, and one the caller may not see look identical.
- State that limits writes (an archived node or a paused status page, say) removes the write capabilities from the scope at resolve time, so read-only is enforced in one place.
- Capabilities form a small closed set. Some are human-only and never granted to automated principals.
- The scope is also what audit logging reads, so authorization and audit see the same value.

For v1per the scope is an opaque `NodeScope`, the node in the tree plus the capabilities the caller holds there through direct or inherited grants. Monitor reads, heartbeat history, incident and status page writes, and exports all take one as a parameter. Code that has no scope cannot call them, and the compiler says so.

```rust
pub struct NodeScope { /* private fields */ }
pub async fn resolve(db, principal: &Principal, node: NodeId) -> Result<NodeScope, ScopeRefusal>;
impl NodeScope { pub fn can(&self, c: Capability) -> bool; }
```

## Control database boundary

The control database (accounts, nodes, grants, monitors, status pages, incidents, settings, revisions, idempotency records, event logs, the probe result inbox) uses one write path and a separate read path:

- Writes are serialized through one writer connection or pool of size one. Each command opens an explicit transaction that takes the write lock immediately.
- Reads use a pool of their own. A query takes a read transaction so its snapshot is consistent.
- WAL mode, foreign keys and a bounded busy timeout are set in one place at connection setup, not per call site.

Snapshot guarantees in this pattern apply inside this one database. They do not extend across a second engine: heartbeats in DuckDB are read in their own snapshot, and a query that joins both states which database each part came from.

## Topics, cursors and subscriptions

A topic is a named, ordered log of state events. Its string form is also its storage key, for example `instance`, `node:<id>`, `inbox:<user id>`. Each event has a sequence number starting at 1, a kind string, a JSON payload, a timestamp, and optionally the entity revision it produced.

A cursor is an opaque string of the form `<topic>@<seq>`. It is parsed strictly. Because it carries its topic, a cursor from one topic cannot be used on another. The browser stores it and hands it back, nothing more.

Subscribing: `GET /api/subscribe?topic=<entry>&topic=<entry>...`, answered as one server-sent event stream (`text/event-stream`, one JSON frame per message, with keep-alive comments) for every listed topic. The browser opens it with `EventSource`, which sends the session cookie.

- Each `topic` entry is a topic (`instance`), which starts at the current head, or a cursor (`node:7@42`), which starts after that sequence. Values are URL-encoded as usual: `topic=node%3A7%4042`.
- One stream takes 1 to 32 distinct topics. A repeated topic, an unknown topic, a bad sequence or a 33rd topic is `400 malformed_request`. A screen that needs more opens a second stream.
- The older single-topic form `topic=<topic>&cursor=<topic>@<seq>` still works: each `cursor` parameter must name a listed topic that has no cursor of its own.
- Topics cannot be added to a running stream. To follow a different set, close the stream and open a new one with each topic's last cursor; nothing is missed or repeated.

Before opening the stream, the server:

- Requires a session and refuses a foreign `Origin` header. A same-origin `EventSource` sends none, so a missing header is allowed.
- Authorizes every topic. An instance topic needs any signed-in person. A resource topic needs a resolvable scope on that resource. A personal topic needs that person. Any refusal answers like an unknown resource, and one refused topic refuses the whole request.

Public status page streams are a separate route with the same frames, without `topic`, for the page's one topic. They check the page's access mode instead of a session and carry only what that page's audience may see (milestone 09).

Frames the server sends, as a tagged union (`TopicFrame` in the generated types). Every frame names the topic it belongs to; cursors and heads are on that topic.

```ts
type Frame = { topic: string } & (
  | { kind: 'subscribed'; head: Cursor; cursor: Cursor }
  | { kind: 'events'; cursor: Cursor; events: Event[] }
  | { kind: 'resync'; cursor: Cursor; reason: 'unavailable' }
  | { kind: 'expired'; head: Cursor }
)
```

For example, `GET /api/subscribe?topic=instance%405&topic=monitors%3A3` opens with:

```text
data: {"topic":"instance","kind":"subscribed","head":"instance@7","cursor":"instance@5"}

data: {"topic":"monitors:3","kind":"subscribed","head":"monitors:3@12","cursor":"monitors:3@12"}

data: {"topic":"instance","kind":"events","cursor":"instance@7","events":[{"seq":6,"kind":"instance.renamed"},{"seq":7,"kind":"instance.renamed"}]}
```

Delivery rules:

- The stream first sends `subscribed` for every topic in request order, then `expired` for any topic whose cursor is past its head.
- Per topic, after `subscribed`, events arrive in sequence order with no gap and no duplicate. Each `events` frame's cursor is the position after its last event. Frames of different topics interleave in no fixed order.
- Catch-up reads each log in pages (500 rows) from its cursor. Topics with pages left take turns, so one long catch-up does not hold up the others. A semaphore (2 at a time) limits how many replay pages are read and serialized across the instance. The permit covers reading and serializing, never the stream write.
- When a topic's catch-up reaches the head, it joins live delivery. A wake signal after commit makes the stream re-read that topic from its cursor. Publishers never queue frames for a subscriber: a stream reads the log only when it can write, so a slow browser holds up nobody but itself.
- `expired` is sent when the cursor is newer than the log. The client must fetch a fresh snapshot for that topic.
- `resync` means: subscribe again from the cursor in the frame, nothing after it was delivered.
- `resync` and `expired` end their topic only. The stream goes on with the other topics and closes when every topic has ended, or when the server stops. To pick an ended topic up again, the client reopens the stream with its full set.

Authorization is re-checked, not assumed. Before each frame and every 5 seconds while idle, the server resolves the session and the topic's scope again. When either is gone it sends `resync` with reason `unavailable` and the topic's last delivered cursor for each topic it affects. The frame that triggered the check is not delivered. A failed re-check read counts as a refusal, and the client reconnects. A sign-out therefore ends every topic and with them the stream.

## Typed client

The web app has exactly one transport class. Pages and components never call `fetch`.

Public surface:

- `command(name, args, { idempotencyKey?, signal? })` returns the result with `revision`. It adds a fresh UUID key when the caller gives none. Reusing a key repeats the same intent and returns the stored answer.
- `query(name, args, { signal? })` returns a snapshot with `revision` and `cursor`. It serializes arguments into the query string.
- `subscribeUrl(topic, cursor)` returns the `/api/subscribe` address for an `EventSource`.
- `upload(path, blob, signal?)` posts raw bytes, never inside a JSON body.
- `setAuthHandler` and `observe` hooks. The auth handler is called with the operation name on a 401. Observers see every response before parsing, which is where a version mismatch check lives.

The types come from the generated registries:

```ts
type CommandName = keyof Commands
type CommandArgs<N extends CommandName> = Commands[N]['args']
type CommandResult<N extends CommandName> = { revision: number } & Commands[N]['result']
```

Behavior:

- Every call is same-origin with the session cookie. Every call also sends a fixed request-marker header. The server requires it on mutations, together with a same-origin `Origin`, which a cross-site form or image cannot produce.
- Cancellation is an `AbortSignal` passed through to `fetch`.
- A non-2xx answer throws one error type: an `Error` carrying `status`, the parsed error body (`error_code`, `message`, optional field `errors`, optional `current`) and the response. Screens branch on `error_code`, never on `message`.
- Error bodies that are not JSON (a proxy HTML page) are cut to 500 characters so a large page does not become the message. An empty status text, as under HTTP/2, falls back to `HTTP <status>`.
- A 204 answer is read as an empty object.

Resource modules sit above this client. Each groups the operations one area needs and gives callers named functions. Transport code owns network calls and nothing else does.

## Live snapshot adapter

Two composables handle live data.

The subscription composable registers its topic on the tab's single multi-topic stream (one stream per 32 topics), which reopens with every topic's last cursor when the set of topics changes. Its inputs are a topic, a cursor from the snapshot on screen, an events callback, and a reset callback. It opens the stream only once the cursor is known, so nothing committed between the snapshot and the subscription is missed. It then behaves as follows:

- It keeps the last cursor the server handed out and resumes from it after a dropped connection. A reconnect replays, it does not refetch.
- On `resync` it closes and reopens at the cursor in the frame.
- On `expired` it closes, clears its position, calls the reset callback, and opens from the head.
- On an unexpected close it retries with a delay that doubles from 1 second up to 30 seconds. A `subscribed` frame resets the delay.
- It exposes `connected` (false while opening and after a drop) and `dropped` (true only from an unexpected close until the next subscribe), so a screen can tell connecting from lost.

The live snapshot composable sits on top. It takes a topic, a `load` function that reads a snapshot query, a `key` that identifies what is loaded, and an optional `affects` predicate over events.

- It owns the snapshot and its revision, not the query cache. A replay gap is a reset, never a cache miss.
- Events arrive in order. An event with a sequence at or below the snapshot revision is already in the snapshot and is skipped. An event past it that `affects` the snapshot marks it stale.
- A batch of events triggers at most one reload. Reads never overlap: an event that arrives during a read schedules exactly one more read after it.
- A change of `key` bumps a generation counter, drops the data, and loads the new key. Results from older generations are discarded.
- Reset from the subscription (expired) triggers a reload.
- It returns the data, error, loading flag, `connected`, `dropped` and a manual `reload`.

The adapter does not apply event deltas to build state. Events say what moved, and the answer is to read the snapshot again. That is why it suits cheap, current resources like a monitor's state, open incidents, probe agent health and import progress, and why it must not be used to rebuild uptime totals. Uptime and response time history use a normal query cache keyed by their full state, and are not refetched for each event.
