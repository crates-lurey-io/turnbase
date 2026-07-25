# Sessions, hosts, and transports

How a defined `Game` becomes playable across a headless CLI, an interactive client, and (eventually)
networked clients, without the game author writing any of that plumbing. This is the layer above
`turnbase-core`; the core's own design lives in `ARCHITECTURE.md`.

## Scope

Built today:

- `turnbase-protocol` - typed request/response wire types.
- `turnbase-session` - the `Session` port, with `LocalSession` (in-memory authority) and
  `FileSession` (stateless, JSON-on-disk).

Deliberately deferred (this document is partly a record of the target shape so we build it
deliberately, not re-derived, when a consumer appears):

- `RemoteSession` and a transport (HTTP, gRPC, MCP, stdio).
- A broker daemon for networked play (the "server").
- A pluggable `Store` beyond the single JSON `FileSession`.
- Peer-to-peer lockstep.

## The port: `Session`

`Session<G>` is a client-side handle, not the host. Its whole surface is:

```rust
fn submit(&mut self, player: PlayerId, request: Request<G::Action>) -> Response<G::View>;
```

Only some implementations hold the authoritative state:

- `LocalSession<G>` owns the `Game` and its state and mutates it in process. This process _is_ the
  host while the value lives. It is serialization-free: the typed `Request`/`Response` pass straight
  through, so in-process play and self-play pay no encode tax (the same principle `turnbase-match`
  follows).
- `FileSession` is the same authority persisted to disk between calls: load, submit one request,
  save back. It serializes only the _state_ (into a self-describing save file that also carries the
  `Game` config), never the action or view, which stay typed.

The request/response envelope is typed (`Request<A>`/`Response<V>`), not opaque bytes. Byte erasure
buys exactly one thing: a transport or store that moves messages without knowing the concrete game.
Nothing built today is game-agnostic, so erasure at the port would be all cost and no benefit. When
a real transport arrives, it does its own typed-to-bytes erasure internally. Keep the port typed;
erase at the edge. (See "the broker" below: this is the same split a production system, Hunk, uses.)

## Roles: host vs client

- **Host**: whichever process currently holds the authoritative `LocalSession`/`FileSession`. Not a
  type, a role.
- **Client**: anything that calls `submit`. The interactive `turnbase-simulator`, the headless CLI,
  and any future remote/MCP client are all clients over the same port.

A single client holding one `LocalSession` already covers a lot without any network: solo play, bots
filling seats, pass-and-play humans alternating on one screen, and simultaneous split-screen humans.
"Single client host" is not "single player." Multiple _client hosts_ (separate processes reaching
one authority) is what the deferred transport adds.

## Deferred: networked play via a broker

The eventual "server" is not a per-game HTTP endpoint. It is a **broker daemon** that lets many
clients reach one running host and tracks many hosts at once. This shape is lifted from Hunk's
`session-broker` (see that project for a production implementation); the mapping to our crates is
one-to-one:

| Broker layer | Owns                                         | Our crate                            |
| ------------ | -------------------------------------------- | ------------------------------------ |
| core         | wire envelopes, selectors, parsing           | `turnbase-protocol`                  |
| engine       | session registry, command routing, lifecycle | `turnbase-session` + a future broker |
| adapter      | concrete HTTP/websocket listeners            | future `turnbase-transport-*`        |

### Two APIs, not one

The broker speaks two protocols:

1. **Host to broker** (persistent connection): the host connects out and `register`s once, then
   streams `snapshot` updates and `heartbeat`s, and answers dispatched commands with a
   `command-result`.
2. **Client to broker** (request/response): three actions, `list` / `get` / `dispatch`. Reads
   (`list`/`get`) and one mutating call (`dispatch`).

This is the "one host, many client APIs, addressed by session id" model. A CLI, an MCP tool, and an
agent all reach one host through the broker.

### Snapshot-push caching (why `Response::State` carries a version)

Client reads are served from the host's most recently **pushed** snapshot, not by round-tripping to
the host; only `dispatch` reaches the host. A read cache like that is only safe if snapshots carry a
monotonic marker so a client can tell fresh from stale. That is exactly why
`Response::State { version, view }` banks a `version: u64` counter now, unused. A counter beats a
timestamp here: monotonic, no clock skew. This is also the real answer to "must every query
re-serialize the whole state?" - in a broker world the host pushes state and the broker serves
reads.

### Session addressing

Sessions are addressable by more than an id: `{ session_id?, session_path?, repo_root? }` with a
fixed precedence (`session_id > session_path > repo_root`) and auto-resolution when only one session
exists. `FileSession` today addresses by path only; a broker would mint ids and let clients target
by id or path. Registration (identity: id, pid, cwd, config, launched-at) is separate from snapshot
(live state), because the selectors live in the registration and must survive a reconnect that
replays it.

### Correlation and lifecycle

Dispatched commands carry a `request_id` the host echoes in its result, so the broker can correlate
async replies against a pending-command map with a per-command timeout. The daemon auto-starts on
first use and idle-shuts-down when it holds no sessions and no pending commands; stale sessions are
pruned by heartbeat TTL; clients reconnect with backoff and re-register.

## Deferred: peer-to-peer lockstep

P2P is a different topology, not another `Session` implementation: there is no single authority, so
request/response does not apply. Every peer runs its own copy and they agree by applying the same
actions in the same order (deterministic lockstep). The precondition - a seeded generator in state,
ordered `ActivePlayers`, no hash-order iteration - is already met by the core (see
`ARCHITECTURE.md`, "Determinism and RNG"), so it is not new ground. A future `PeerChannel<G>` would
broadcast actions (reusing `turnbase-protocol`'s wire types) rather than proxying to a host, and
would need a desync detector (periodic state-hash exchange), since no authority exists to catch
divergence.

## What we did not build yet, and why

- **No `Store` trait.** `FileSession` is the only persistence, and one implementation is not a
  pattern. Extract a trait when a second real backend (sqlite for many concurrent sessions, say)
  exists.
- **No `RemoteSession`/transport.** No client needs one yet. Building it now would guess the
  transport shape before a consumer constrains it.
- **No broker.** Deferred until networked play is a real requirement. This document is the note so
  it gets built as the shape above.
