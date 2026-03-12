# Phase 4: HTTP Endpoints & Wire Protocol

**Goal**: Implement the MeshEnvelope wire format and all MESH HTTP endpoints using axum.
**Duration**: 3-4 days
**Branch**: `feature/4-http`

## Prerequisites

- Phase 1 complete (core types, error types with IntoResponse)
- Phase 2 complete (identity document, signing)
- Phase 3 complete (storage layer)

## Context

MESH defines 4 HTTP endpoints (spec §7.2) plus a well-known identity URL. All messages are wrapped in a `MeshEnvelope` (spec §7.3). The endpoints are added to nellie-rs's existing axum server by providing a `Router` that can be merged.

Endpoints:
- `GET  /.well-known/mesh/identity` — identity document (discovery)
- `GET  /mesh/v1/identity` — identity document (canonical)
- `POST /mesh/v1/announce` — receive publication or revocation from peer
- `POST /mesh/v1/search` — receive and respond to search queries
- `GET  /mesh/v1/peers` — list connected peers

---

## Task 4.1: Wire Protocol Types

### Subtask 4.1.1: MeshEnvelope & Request/Response Types (Single Session)

**Prerequisites**:
- [x] 1.1.2: Core types

**Deliverables**:
- [ ] Implement in `src/envelope.rs`:
  - `MeshEnvelope` struct:
    ```rust
    pub struct MeshEnvelope {
        pub version: String,        // always "mesh/1.0"
        pub r#type: String,         // "publication", "revocation", "search", "searchResponse"
        pub timestamp: i64,         // Unix ms
        pub sender: String,         // sender's node ID
        pub payload: serde_json::Value,
        pub signature: Option<String>,  // base64 signature, required for mutations
    }
    ```
  - `MeshEnvelope::new(sender, msg_type, payload) -> Self` — constructor with auto-timestamp
  - `MeshEnvelope::sign(&mut self, identity: &NodeIdentity) -> MeshResult<()>` — sign the envelope
  - `MeshEnvelope::verify_signature(&self) -> MeshResult<()>` — verify envelope signature
  - Serde with `camelCase` rename for wire compatibility
- [ ] Define request/response types in `src/types.rs` (or extend existing):
  - `PublicationAnnouncement` — matches spec §3.3
    ```rust
    pub struct PublicationAnnouncement {
        pub record_type: RecordType,           // "lesson" or "checkpoint"
        pub signed_lesson: Option<SignedLesson>,
        pub signed_checkpoint: Option<SignedCheckpoint>,
    }
    ```
  - `RevocationAnnouncement` — wraps a `Revocation`
  - `SearchRequest` — matches spec §5.1 (simplified for MVP: no hops/TTL)
    ```rust
    pub struct SearchRequest {
        pub query: String,
        pub record_types: Option<Vec<RecordType>>,
        pub filters: Option<SearchFilters>,
        pub limit: Option<usize>,              // default 20, max 100
        pub request_id: String,
        pub origin: String,                    // originating node ID
    }
    ```
  - `SearchFilters` — optional tag/topic/time filters
  - `SearchResponse` — matches spec §5.3
    ```rust
    pub struct SearchResponse {
        pub request_id: String,
        pub results: Vec<SearchResult>,
        pub truncated: bool,
    }
    ```
  - `SearchResult` — single result with score
    ```rust
    pub struct SearchResult {
        pub record_type: RecordType,
        pub signed_lesson: Option<SignedLesson>,
        pub signed_checkpoint: Option<SignedCheckpoint>,
        pub score: f64,                        // 0.0 - 1.0
        pub trust_score: f64,
        pub via: Option<String>,               // node that provided result
    }
    ```
- [ ] All types derive Serialize, Deserialize with camelCase
- [ ] Unit tests:
  - MeshEnvelope serializes to correct JSON structure
  - MeshEnvelope sign/verify round-trip
  - All request/response types serde round-trip
  - Verify version is always "mesh/1.0"

**Files to Modify**:
- `src/envelope.rs`
- `src/types.rs`

**Success Criteria**:
- [ ] All wire types compile and serialize correctly
- [ ] MeshEnvelope JSON matches spec §7.3 format
- [ ] `cargo test envelope` and `cargo test types` pass

**Verification**:
```bash
cargo test envelope -- --nocapture
cargo test types -- --nocapture
```

---

**Completion Notes**:
- **Implementation**: _(describe what was done)_
- **Files Modified**: _(list)_
- **Tests**: _(X tests passing)_
- **Commit**: `feat(wire): MeshEnvelope and request/response types`

---

### Subtask 4.1.2: Axum Router & Shared State (Single Session)

**Prerequisites**:
- [x] 4.1.1: Wire protocol types
- [x] 3.1.1: Schema migrations (MeshStorage)
- [x] 2.1.1: NodeIdentity

**Deliverables**:
- [ ] Define `MeshState` in `src/http/mod.rs`:
  ```rust
  pub struct MeshState {
      pub identity: Arc<NodeIdentity>,
      pub db: Arc<Mutex<rusqlite::Connection>>,  // shared with nellie-rs
      // Will add PeerManager, TrustManager in later phases
  }
  ```
  - Use `Arc` for thread-safe sharing across axum handlers
  - `Mutex<Connection>` because rusqlite Connection is not Sync
- [ ] Implement `mesh_router(state: MeshState) -> Router`:
  - `GET  /.well-known/mesh/identity` → identity handler
  - `GET  /mesh/v1/identity` → same identity handler
  - `POST /mesh/v1/announce` → announce handler (stub initially)
  - `POST /mesh/v1/search` → search handler (stub initially)
  - `GET  /mesh/v1/peers` → peers handler (stub initially)
  - Pass `MeshState` via axum's `State` extractor
- [ ] Stub handlers that return appropriate placeholder responses (200 with empty JSON or 501 Not Implemented)
- [ ] Unit test: verify router has correct routes defined

**Key Decisions**:
- `mesh_router` returns an `axum::Router` that nellie-rs merges with `.merge()` or `.nest()`
- State uses `Arc` wrapping so the router can be cloned into axum
- Database connection shared with nellie-rs via `Arc<Mutex<Connection>>`

**Files to Modify**:
- `src/http/mod.rs`

**Success Criteria**:
- [ ] `mesh_router()` compiles and returns a valid Router
- [ ] All 5 routes are registered
- [ ] `cargo check` passes

**Verification**:
```bash
cargo check
```

---

**Completion Notes**:
- **Implementation**: _(describe what was done)_
- **Files Modified**: _(list)_
- **Tests**: _(X tests passing)_
- **Commit**: `feat(http): axum router setup with shared state`

---

## Task 4.2: Endpoint Handlers

### Subtask 4.2.1: Identity Endpoint (Single Session)

**Prerequisites**:
- [x] 4.1.2: Router setup
- [x] 2.1.2: Identity document

**Deliverables**:
- [ ] Implement in `src/http/identity.rs`:
  - `pub async fn get_identity(State(state): State<Arc<MeshState>>) -> impl IntoResponse`
    - Calls `state.identity.identity_document(mesh_endpoint)`
    - Returns JSON response with the identity document
    - Sets `Content-Type: application/json`
  - The mesh_endpoint URL should be configurable (stored in MeshState or derived from request)
- [ ] Integration test using `axum::test` helpers:
  - Create a MeshState with a generated identity
  - Send GET to `/mesh/v1/identity`
  - Verify response is 200 with valid JSON
  - Verify response contains correct node_id
  - Verify response contains version "mesh/1.0"
  - Verify self-signature is valid
  - Send GET to `/.well-known/mesh/identity` — same response

**Files to Modify**:
- `src/http/identity.rs`
- `src/http/mod.rs` — wire up handler

**Success Criteria**:
- [ ] GET /mesh/v1/identity returns valid identity document JSON
- [ ] GET /.well-known/mesh/identity returns same document
- [ ] Self-signature in response is verifiable
- [ ] `cargo test http::identity` passes

**Verification**:
```bash
cargo test http::identity -- --nocapture
```

---

**Completion Notes**:
- **Implementation**: _(describe what was done)_
- **Files Modified**: _(list)_
- **Tests**: _(X tests passing)_
- **Commit**: `feat(http): identity endpoint handler`

---

### Subtask 4.2.2: Announce Endpoint (Single Session)

**Prerequisites**:
- [x] 4.1.2: Router setup
- [x] 2.2.2: Record signing/verification
- [x] 3.1.4: Remote records & revocations storage

**Deliverables**:
- [ ] Implement in `src/http/announce.rs`:
  - `pub async fn post_announce(State(state): State<Arc<MeshState>>, Json(envelope): Json<MeshEnvelope>) -> impl IntoResponse`
    - Verify envelope signature
    - Check sender is a known, trusted peer
    - Parse payload as `PublicationAnnouncement` or `RevocationAnnouncement` based on envelope type
    - For publication:
      1. Verify signed record signature
      2. Check not already revoked
      3. Store in mesh_remote_records
      4. Return 200 OK
    - For revocation:
      1. Verify revocation signature
      2. Verify revoker matches original publisher
      3. Store revocation, delete cached record
      4. Return 200 OK
    - Return appropriate MeshError for failures (401 untrusted, 400 invalid, etc.)
- [ ] Integration tests:
  - Send valid publication announcement → 200, record stored
  - Send publication from untrusted peer → 401
  - Send publication with invalid signature → 401
  - Send revocation for existing record → 200, record deleted
  - Send revocation from non-publisher → 401
  - Send duplicate publication of revoked record → rejected

**Key Design Notes**:
- The announce handler is the core ingestion point — security-critical
- ALL records must pass signature verification before storage (spec §10.1 rule 3)
- Trust check happens before any processing

**Files to Modify**:
- `src/http/announce.rs`
- `src/http/mod.rs` — wire up handler

**Success Criteria**:
- [ ] Valid publications from trusted peers are accepted and stored
- [ ] Invalid signatures are rejected with 401
- [ ] Untrusted peers are rejected with 401
- [ ] Revocations work correctly (delete cached, store revocation)
- [ ] `cargo test http::announce` passes

**Verification**:
```bash
cargo test http::announce -- --nocapture
```

---

**Completion Notes**:
- **Implementation**: _(describe what was done)_
- **Files Modified**: _(list)_
- **Tests**: _(X tests passing)_
- **Commit**: `feat(http): announce endpoint for publications and revocations`

---

### Subtask 4.2.3: Search Endpoint (Single Session)

**Prerequisites**:
- [x] 4.1.2: Router setup
- [x] 3.1.4: Remote records storage (search_remote_records)

**Deliverables**:
- [ ] Implement in `src/http/search.rs`:
  - `pub async fn post_search(State(state): State<Arc<MeshState>>, Json(envelope): Json<MeshEnvelope>) -> impl IntoResponse`
    - Verify envelope signature (optional for search — spec says signature optional for non-mutations)
    - Check sender is a known peer (doesn't need to be trusted for public search)
    - Parse payload as `SearchRequest`
    - Query local public records (mesh_remote_records + own public records)
    - Apply filters (record_type, tags, topics, limit)
    - Clamp limit to max 100
    - Build `SearchResponse` with results and scores
    - Wrap in MeshEnvelope and return
  - Score calculation for MVP: basic text match relevance (0.0-1.0)
  - Trust score: 1.0 for direct trust (single-hop MVP)
- [ ] Integration tests:
  - Send search request → get results
  - Search with record_type filter → only matching types returned
  - Search with limit → results capped
  - Search with no matches → empty results, truncated=false
  - Verify response is wrapped in MeshEnvelope

**Files to Modify**:
- `src/http/search.rs`
- `src/http/mod.rs` — wire up handler

**Success Criteria**:
- [ ] Search returns matching records from local storage
- [ ] Filters work correctly
- [ ] Limit is enforced (max 100)
- [ ] Response follows SearchResponse format
- [ ] `cargo test http::search` passes

**Verification**:
```bash
cargo test http::search -- --nocapture
```

---

**Completion Notes**:
- **Implementation**: _(describe what was done)_
- **Files Modified**: _(list)_
- **Tests**: _(X tests passing)_
- **Commit**: `feat(http): search endpoint handler`

---

### Subtask 4.2.4: Peers Endpoint (Single Session)

**Prerequisites**:
- [x] 4.1.2: Router setup
- [x] 3.1.3: Peer storage

**Deliverables**:
- [ ] Implement in `src/http/peers.rs`:
  - `pub async fn get_peers(State(state): State<Arc<MeshState>>) -> impl IntoResponse`
    - List all peers from storage
    - Return JSON array of peer info (node_id, endpoint, trust_level, last_seen, connected_since)
    - Do NOT include private details like internal state
- [ ] Integration test:
  - Add peers to storage, request GET /mesh/v1/peers, verify response lists them

**Files to Modify**:
- `src/http/peers.rs`
- `src/http/mod.rs` — wire up handler

**Success Criteria**:
- [ ] GET /mesh/v1/peers returns peer list as JSON array
- [ ] Response matches PeerConnection serialization format
- [ ] `cargo test http::peers` passes

**Verification**:
```bash
cargo test http::peers -- --nocapture
```

---

**Completion Notes**:
- **Implementation**: _(describe what was done)_
- **Files Modified**: _(list)_
- **Tests**: _(X tests passing)_
- **Commit**: `feat(http): peers endpoint handler`

---

## Phase 4 Complete — Squash Merge

- [ ] All subtasks complete (4.1.1 – 4.2.4)
- [ ] `cargo test` — all tests pass
- [ ] `cargo clippy -- -D warnings` — no warnings
- [ ] `cargo fmt --check` — no formatting issues
- [ ] Squash merge to main:
  ```bash
  git checkout main && git merge --squash feature/4-http
  git commit -m "feat: phase 4 — HTTP endpoints and wire protocol"
  git push origin main
  git branch -d feature/4-http
  ```

---

*Previous: [Phase 3 — Storage Layer](PHASE_3.md) | Next: [Phase 5 — Peer Management & Trust](PHASE_5.md)*
