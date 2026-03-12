# Phase 8: MCP Tools & Integration

**Goal**: Implement the 6 new MCP tools that agents use for federation, create the MeshNode integration struct, and write end-to-end tests.
**Duration**: 3-4 days
**Branch**: `feature/8-mcp`

## Prerequisites

- All previous phases complete (1-7)

## Context

This is the final phase. It ties everything together with:
1. Six MCP tools that agents use to interact with federation
2. A `MeshNode` struct that wires up all components
3. End-to-end tests proving two nodes can federate

The MCP tools are the agent-facing interface. They must be simple, clear, and return useful information. Each tool maps to a high-level federation operation.

MCP tools from the PROJECT_BRIEF:
- `mesh_publish` — publish a lesson or checkpoint (set visibility, sign, announce to peer)
- `mesh_search` — search local + peer records, return merged results
- `mesh_peers` — list, add, remove peers
- `mesh_trust` — add/remove trust for a peer
- `mesh_revoke` — revoke a previously published record
- `mesh_status` — federation status (identity, peer count, published record count)

---

## Task 8.1: MCP Tool Implementations

### Subtask 8.1.1: mesh_status & mesh_peers Tools (Single Session)

**Prerequisites**:
- [x] 5.1.1: PeerManager
- [x] 3.1.2: Identity storage

**Deliverables**:
- [ ] Create MCP tool handler module (e.g., `src/tools.rs` or integrate into `src/lib.rs`):
  - Define tool handler functions that take parameters and return JSON results
  - These functions will be registered by nellie-rs as MCP tools
- [ ] `mesh_status` tool:
  - **Parameters**: none
  - **Returns**: JSON with:
    - `node_id` — this node's ID
    - `fingerprint` — this node's fingerprint
    - `peer_count` — number of configured peers
    - `trusted_peer_count` — number of trusted peers
    - `published_record_count` — number of records this node has published (local public/unlisted records)
    - `cached_remote_records` — number of cached records from peers
    - `mesh_endpoint` — this node's MESH endpoint URL (if configured)
  - Implementation: query storage for counts, identity for node info
- [ ] `mesh_peers` tool:
  - **Parameters**:
    - `action: String` — "list", "add", or "remove"
    - `node_id: Option<String>` — required for add/remove
    - `endpoint: Option<String>` — required for add
  - **Returns**: JSON with:
    - For "list": array of peer objects (node_id, endpoint, trust_level, last_seen, connected_since)
    - For "add": `{"added": true, "node_id": "..."}` or error
    - For "remove": `{"removed": true, "node_id": "..."}` or error
  - Implementation: delegates to PeerManager
- [ ] Unit tests:
  - mesh_status returns correct counts
  - mesh_peers list returns all peers
  - mesh_peers add creates a new peer
  - mesh_peers remove deletes a peer
  - mesh_peers add without endpoint — error
  - mesh_peers remove nonexistent — error

**Files to Create**:
- `src/tools.rs`

**Files to Modify**:
- `src/lib.rs` — add module, re-export

**Success Criteria**:
- [ ] mesh_status returns accurate node information
- [ ] mesh_peers CRUD operations work correctly
- [ ] `cargo test tools` passes

**Verification**:
```bash
cargo test tools -- --nocapture
```

---

**Completion Notes**:
- **Implementation**: _(describe what was done)_
- **Files Created/Modified**: _(list)_
- **Tests**: _(X tests passing)_
- **Commit**: `feat(tools): mesh_status and mesh_peers MCP tools`

---

### Subtask 8.1.2: mesh_trust Tool (Single Session)

**Prerequisites**:
- [x] 5.1.2: TrustManager
- [x] 8.1.1: Tool handler module

**Deliverables**:
- [ ] `mesh_trust` tool in `src/tools.rs`:
  - **Parameters**:
    - `action: String` — "add", "remove", or "list"
    - `node_id: Option<String>` — required for add/remove
  - **Returns**:
    - For "add": `{"trusted": true, "node_id": "..."}` and optionally trigger identity verification
    - For "remove": `{"trusted": false, "node_id": "..."}`
    - For "list": array of trusted peer objects
  - On "add": after setting trust, automatically attempt to verify the peer's identity
    - If verification succeeds: include `"identity_verified": true` in response
    - If verification fails: still set trust, but include warning `"identity_verified": false, "warning": "..."` — let the operator decide
- [ ] Unit tests:
  - Trust add sets trust level
  - Trust remove clears trust level
  - Trust list returns only trusted peers
  - Trust add on nonexistent peer — error with helpful message ("add peer first")

**Files to Modify**:
- `src/tools.rs`

**Success Criteria**:
- [ ] Trust operations work correctly
- [ ] Identity verification is attempted on trust add
- [ ] Helpful error messages for common mistakes
- [ ] `cargo test tools::trust` passes

**Verification**:
```bash
cargo test tools -- --nocapture
```

---

**Completion Notes**:
- **Implementation**: _(describe what was done)_
- **Files Modified**: _(list)_
- **Tests**: _(X tests passing)_
- **Commit**: `feat(tools): mesh_trust MCP tool with identity verification`

---

### Subtask 8.1.3: mesh_publish Tool (Single Session)

**Prerequisites**:
- [x] 6.1.1: Publication flow (Publisher)
- [x] 8.1.1: Tool handler module

**Deliverables**:
- [ ] `mesh_publish` tool in `src/tools.rs`:
  - **Parameters**:
    - `record_id: String` — ID of the lesson or checkpoint to publish
    - `record_type: String` — "lesson" or "checkpoint"
    - `visibility: Option<String>` — "public" or "unlisted" (default: "public")
    - `topics: Option<Vec<String>>` — optional categorization tags
  - **Returns**: JSON with:
    - `published: true`
    - `record_id` — the published record's ID
    - `record_type` — "lesson" or "checkpoint"
    - `visibility` — the visibility that was set
    - `signature` — summary of signature (node_id, timestamp)
    - `announced_to` — array of peer node_ids that were notified, with success/failure per peer
  - Implementation:
    1. Fetch the record from nellie-rs's database (lessons or checkpoints table by ID)
    2. Convert to serde_json::Value
    3. Call Publisher::publish_lesson() or publish_checkpoint()
    4. Update the record's visibility in the local table
    5. Return result
  - Error if record not found, or if visibility is "private" (can't publish as private)
- [ ] Unit tests:
  - Publish lesson — returns signed record info
  - Publish checkpoint — returns signed record info
  - Publish with "private" visibility — error
  - Publish nonexistent record — error

**Key Design Note**:
- This tool needs access to nellie-rs's lessons/checkpoints tables to fetch the record content. The MeshNode struct (subtask 8.2.1) will provide this via a callback or trait that nellie-rs implements.
- For now, implement with a `RecordFetcher` trait that mesh-node defines and nellie-rs will implement:
  ```rust
  pub trait RecordFetcher: Send + Sync {
      fn fetch_lesson(&self, id: &str) -> MeshResult<Option<serde_json::Value>>;
      fn fetch_checkpoint(&self, id: &str) -> MeshResult<Option<serde_json::Value>>;
      fn set_visibility(&self, id: &str, record_type: RecordType, visibility: Visibility) -> MeshResult<()>;
  }
  ```

**Files to Modify**:
- `src/tools.rs`
- `src/types.rs` or `src/lib.rs` — RecordFetcher trait

**Success Criteria**:
- [ ] Publish tool produces correct output format
- [ ] RecordFetcher trait is defined for nellie-rs integration
- [ ] `cargo test tools::publish` passes

**Verification**:
```bash
cargo test tools -- --nocapture
```

---

**Completion Notes**:
- **Implementation**: _(describe what was done)_
- **Files Modified**: _(list)_
- **Tests**: _(X tests passing)_
- **Commit**: `feat(tools): mesh_publish MCP tool with RecordFetcher trait`

---

### Subtask 8.1.4: mesh_revoke Tool (Single Session)

**Prerequisites**:
- [x] 6.2.1: Revocation flow (Revoker)
- [x] 8.1.1: Tool handler module

**Deliverables**:
- [ ] `mesh_revoke` tool in `src/tools.rs`:
  - **Parameters**:
    - `record_id: String` — ID of the record to revoke
    - `record_type: String` — "lesson" or "checkpoint"
    - `reason: Option<String>` — "outdated", "incorrect", "private", or "other"
  - **Returns**: JSON with:
    - `revoked: true`
    - `record_id`
    - `record_type`
    - `reason`
    - `announced_to` — peers notified with per-peer success/failure
  - Implementation:
    1. Call Revoker::revoke()
    2. Update local record visibility back to "private" (via RecordFetcher)
    3. Return result
  - Error if this node didn't publish the record (can only revoke own publications)
- [ ] Unit tests:
  - Revoke own publication — success
  - Revoke non-own record — error
  - Revoke already-revoked — error or idempotent success

**Files to Modify**:
- `src/tools.rs`

**Success Criteria**:
- [ ] Revoke tool produces correct output
- [ ] Only own publications can be revoked
- [ ] `cargo test tools::revoke` passes

**Verification**:
```bash
cargo test tools -- --nocapture
```

---

**Completion Notes**:
- **Implementation**: _(describe what was done)_
- **Files Modified**: _(list)_
- **Tests**: _(X tests passing)_
- **Commit**: `feat(tools): mesh_revoke MCP tool`

---

### Subtask 8.1.5: mesh_search Tool (Single Session)

**Prerequisites**:
- [x] 7.1.3: Federated search (FederatedSearch)
- [x] 8.1.1: Tool handler module

**Deliverables**:
- [ ] `mesh_search` tool in `src/tools.rs`:
  - **Parameters**:
    - `query: String` — search query text
    - `record_types: Option<Vec<String>>` — filter: ["lesson"], ["checkpoint"], or both
    - `tags: Option<Vec<String>>` — filter by tags
    - `limit: Option<usize>` — max results (default 20, max 100)
  - **Returns**: JSON with:
    - `results` — array of search results, each with:
      - `record_type` — "lesson" or "checkpoint"
      - `record` — the lesson or checkpoint content (from signed record)
      - `score` — combined relevance score
      - `source` — "local" or peer node_id
      - `publisher` — publisher's node_id
      - `published_at` — publication timestamp
    - `total_results` — count
    - `truncated` — boolean
    - `peers_queried` — number of peers searched
    - `peers_responded` — number that responded
  - Implementation:
    1. Call FederatedSearch::search()
    2. Transform SearchResponse into agent-friendly format
    3. Extract readable content from SignedLesson/SignedCheckpoint for display
- [ ] Unit tests:
  - Search returns local results
  - Search with type filter
  - Search with limit
  - Empty search returns empty results
  - Results include source information

**Files to Modify**:
- `src/tools.rs`

**Success Criteria**:
- [ ] Search tool returns correctly formatted results
- [ ] Filters work (record_type, limit)
- [ ] Results include source and publisher information
- [ ] `cargo test tools::search` passes

**Verification**:
```bash
cargo test tools -- --nocapture
```

---

**Completion Notes**:
- **Implementation**: _(describe what was done)_
- **Files Modified**: _(list)_
- **Tests**: _(X tests passing)_
- **Commit**: `feat(tools): mesh_search MCP tool with federated results`

---

## Task 8.2: Integration

### Subtask 8.2.1: MeshNode Integration Struct (Single Session)

**Prerequisites**:
- [x] All tool subtasks (8.1.1 - 8.1.5)
- [x] All component subtasks from phases 2-7

**Deliverables**:
- [ ] Implement `MeshNode` in `src/lib.rs` — the main public API struct:
  ```rust
  pub struct MeshNode {
      identity: Arc<NodeIdentity>,
      storage: Arc<MeshStorage>,         // or Arc<Mutex<Connection>>
      peer_manager: Arc<PeerManager>,
      trust_manager: Arc<TrustManager>,
      publisher: Arc<Publisher>,
      revoker: Arc<Revoker>,
      search: Arc<FederatedSearch>,
      http_client: reqwest::Client,
  }
  ```
  - `MeshNode::new(conn: Arc<Mutex<Connection>>, config: MeshConfig) -> MeshResult<Self>`
    - Run migrations
    - Load or generate identity (generate on first run, load on subsequent)
    - Initialize all components
    - Build reqwest client with appropriate timeouts
  - `MeshNode::router(&self) -> Router` — returns the axum Router for merging
  - `MeshNode::identity(&self) -> &NodeIdentity`
  - `MeshNode::node_id(&self) -> &str`
  - Accessors for all MCP tool handler functions
  - `MeshNode::spawn_background_tasks(&self) -> Vec<JoinHandle<()>>`
    - Start health check loop
    - Start any other background tasks
  - `MeshNode::shutdown(&self)` — clean shutdown of background tasks
- [ ] Define `MeshConfig`:
  ```rust
  pub struct MeshConfig {
      pub mesh_endpoint: String,        // this node's public MESH URL
      pub health_check_interval: Duration,  // default: 5 min
  }
  ```
- [ ] Integration test:
  - Create MeshNode with in-memory SQLite
  - Verify identity is generated on first run
  - Verify identity is loaded (not regenerated) on second run
  - Verify router has all expected routes

**Key Decisions**:
- MeshNode is the single entry point for nellie-rs integration
- Identity is auto-generated on first run and persisted — operators don't need to manually create keys
- `MeshConfig.mesh_endpoint` is required — this is the tunnel URL where peers can reach this node

**Files to Modify**:
- `src/lib.rs`

**Success Criteria**:
- [ ] MeshNode initializes correctly with all components
- [ ] Identity is persisted and reloaded across restarts
- [ ] Router has all MESH endpoints
- [ ] `cargo test mesh_node` passes

**Verification**:
```bash
cargo test mesh_node -- --nocapture
```

---

**Completion Notes**:
- **Implementation**: _(describe what was done)_
- **Files Modified**: _(list)_
- **Tests**: _(X tests passing)_
- **Commit**: `feat(mesh): MeshNode integration struct with auto-identity`

---

### Subtask 8.2.2: End-to-End Federation Test (Single Session)

**Prerequisites**:
- [x] 8.2.1: MeshNode struct

**Deliverables**:
- [ ] Create `tests/federation_test.rs` — integration test with two in-process nodes:
  1. **Setup**: Create two MeshNode instances (Node A, Node B) with separate in-memory SQLite databases and separate identities
  2. **Start servers**: Bind each to a random localhost port using `axum::Server`
  3. **Add peers**: Node A adds Node B as peer (and vice versa) using localhost URLs
  4. **Trust**: Both nodes trust each other
  5. **Verify identity**: Node A verifies Node B's identity (and vice versa) — should succeed
  6. **Publish lesson**: Node A publishes a test lesson with visibility=public
  7. **Verify receipt**: Node B's mesh_remote_records contains the lesson
  8. **Verify signature**: The stored record has a valid signature
  9. **Publish checkpoint**: Node A publishes a test checkpoint
  10. **Verify receipt**: Node B receives the checkpoint
  11. **Search**: Node B searches for the lesson — finds it
  12. **Federated search**: Node B initiates federated search querying Node A — gets results
  13. **Revoke**: Node A revokes the lesson
  14. **Verify revocation**: Node B no longer has the lesson cached, revocation is stored
  15. **Re-announce blocked**: Attempting to re-announce the revoked lesson to Node B — rejected
- [ ] Test should be `#[tokio::test]` with reasonable timeouts
- [ ] Each step should have clear assertions with descriptive messages
- [ ] Clean shutdown of both servers after test

**Key Decisions**:
- Use `tokio::net::TcpListener::bind("127.0.0.1:0")` to get random available ports
- This test exercises the full publish → receive → search → revoke lifecycle
- This is the acceptance test for the entire mesh-node crate

**Success Criteria**:
- [ ] Full federation lifecycle works between two in-process nodes
- [ ] All assertions pass: publish, receive, search, revoke
- [ ] Test completes in < 30 seconds
- [ ] `cargo test federation_test` passes

**Verification**:
```bash
cargo test federation_test -- --nocapture
```

---

**Completion Notes**:
- **Implementation**: _(describe what was done)_
- **Files Created**: _(list)_
- **Tests**: _(X assertions passing)_
- **Commit**: `test: end-to-end federation test with two in-process nodes`

---

## Phase 8 Complete — Squash Merge

- [ ] All subtasks complete (8.1.1 – 8.2.2)
- [ ] `cargo test` — ALL tests pass (unit + integration + federation)
- [ ] `cargo clippy -- -D warnings` — no warnings
- [ ] `cargo fmt --check` — no formatting issues
- [ ] Test coverage >= 80% (check with `cargo tarpaulin` or similar)
- [ ] Squash merge to main:
  ```bash
  git checkout main && git merge --squash feature/8-mcp
  git commit -m "feat: phase 8 — MCP tools and end-to-end federation"
  git push origin main
  git branch -d feature/8-mcp
  ```

---

## MVP Complete Checklist

After Phase 8, verify all success criteria from PROJECT_BRIEF.md:

- [ ] Two nellie-rs instances on separate Tailscale networks can connect via Cloudflare Tunnel
- [ ] Node A can publish a lesson, Node B receives and stores it
- [ ] Node A can publish a checkpoint, Node B receives and stores it
- [ ] Node B can search Node A's public records via `mesh_search`
- [ ] Node A can revoke a published record, Node B deletes its cached copy
- [ ] Existing nellie-rs MCP tools (`add_lesson`, `search_lessons`, etc.) work unchanged
- [ ] Schema migration preserves all existing data
- [ ] All remote records have verified Ed25519 signatures before storage
- [ ] Test coverage >= 80%

---

*Previous: [Phase 7 — Federated Search](PHASE_7.md)*

*Congratulations — mesh-node MVP is complete. See DEVELOPMENT_PLAN.md for v2 roadmap.*
