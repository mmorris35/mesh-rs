# Phase 5: Peer Management & Trust

**Goal**: Implement peer lifecycle management, binary trust model, identity verification, and health checks.
**Duration**: 2-3 days
**Branch**: `feature/5-peers`

## Prerequisites

- Phase 3 complete (peer/trust storage)
- Phase 2 complete (identity verification)

## Context

MESH MVP uses direct trust (N=2): you either explicitly trust a peer or you don't. Peers are added manually by exchanging node IDs and tunnel URLs out-of-band. After adding a peer, the node verifies the peer's identity by fetching their identity document and confirming the self-signature.

The bootstrap flow (from PROJECT_BRIEF.md):
1. Both operators add the other as a peer: `mesh_peers add <nodeId> <tunnelUrl>`
2. Each operator trusts the other: `mesh_trust add <nodeId>`
3. Nodes verify identity by fetching `/.well-known/mesh/identity`
4. Federation is live

---

## Task 5.1: Peer & Trust Management

### Subtask 5.1.1: PeerManager (Single Session)

**Prerequisites**:
- [x] 3.1.3: Peer storage CRUD

**Deliverables**:
- [ ] Implement `PeerManager` in `src/peer.rs`:
  - `PeerManager::new(db: Arc<Mutex<Connection>>, http_client: reqwest::Client) -> Self`
  - `PeerManager::add_peer(&self, node_id: &str, endpoint: &str) -> MeshResult<()>`
    - Validate endpoint is a valid URL
    - Store in mesh_peers via storage layer
    - Does NOT automatically trust — trust is a separate action
  - `PeerManager::remove_peer(&self, node_id: &str) -> MeshResult<()>`
    - Remove peer and all associated remote records
  - `PeerManager::list_peers(&self) -> MeshResult<Vec<PeerConnection>>`
  - `PeerManager::get_peer(&self, node_id: &str) -> MeshResult<Option<PeerConnection>>`
  - `PeerManager::is_known_peer(&self, node_id: &str) -> MeshResult<bool>`
- [ ] Unit tests:
  - Add peer, verify it appears in list
  - Remove peer, verify it's gone
  - Add peer with invalid URL — error
  - Get nonexistent peer — None

**Files to Modify**:
- `src/peer.rs`
- `src/lib.rs` — re-export

**Success Criteria**:
- [ ] CRUD operations work correctly
- [ ] URL validation catches malformed endpoints
- [ ] `cargo test peer` passes

**Verification**:
```bash
cargo test peer -- --nocapture
```

---

**Completion Notes**:
- **Implementation**: _(describe what was done)_
- **Files Modified**: _(list)_
- **Tests**: _(X tests passing)_
- **Commit**: `feat(peer): PeerManager with add/remove/list operations`

---

### Subtask 5.1.2: TrustManager (Single Session)

**Prerequisites**:
- [x] 5.1.1: PeerManager
- [x] 3.1.3: Peer storage (trust level operations)

**Deliverables**:
- [ ] Implement `TrustManager` in `src/trust.rs`:
  - `TrustManager::new(db: Arc<Mutex<Connection>>) -> Self`
  - `TrustManager::add_trust(&self, node_id: &str) -> MeshResult<()>`
    - Sets trust_level to 'full' for the given peer
    - Error if peer doesn't exist (must add peer first)
  - `TrustManager::remove_trust(&self, node_id: &str) -> MeshResult<()>`
    - Sets trust_level to 'none'
  - `TrustManager::is_trusted(&self, node_id: &str) -> MeshResult<bool>`
    - Returns true if trust_level == 'full'
  - `TrustManager::list_trusted(&self) -> MeshResult<Vec<PeerConnection>>`
    - Returns only trusted peers
- [ ] Unit tests:
  - Add trust to existing peer — is_trusted returns true
  - Remove trust — is_trusted returns false
  - Add trust to nonexistent peer — error
  - list_trusted only returns trusted peers

**Key Decisions**:
- MVP trust is binary: `Full` or `None`. No `Limited` level yet.
- Trust requires the peer to exist first (add peer, then trust)
- Transitive trust is v2 (web of trust with configurable depth)

**Files to Modify**:
- `src/trust.rs`
- `src/lib.rs` — re-export

**Success Criteria**:
- [ ] Binary trust model works correctly
- [ ] Trust requires existing peer
- [ ] `cargo test trust` passes

**Verification**:
```bash
cargo test trust -- --nocapture
```

---

**Completion Notes**:
- **Implementation**: _(describe what was done)_
- **Files Modified**: _(list)_
- **Tests**: _(X tests passing)_
- **Commit**: `feat(trust): binary trust manager for MVP`

---

### Subtask 5.1.3: Peer Identity Verification (Single Session)

**Prerequisites**:
- [x] 5.1.1: PeerManager (has HTTP client)
- [x] 2.1.2: Identity document verification

**Deliverables**:
- [ ] Implement in `src/peer.rs`:
  - `PeerManager::verify_peer_identity(&self, node_id: &str) -> MeshResult<IdentityDocument>`
    - Fetch `{endpoint}/.well-known/mesh/identity` via reqwest
    - Parse response as `IdentityDocument`
    - Verify self-signature using `IdentityDocument::verify()`
    - Verify `identity.node_id` matches the `node_id` we have stored for this peer
    - If mismatch: return `MeshError::IdentityError` with detail
    - If valid: update `last_seen` timestamp, return document
  - `PeerManager::verify_all_peers(&self) -> MeshResult<Vec<(String, Result<IdentityDocument, MeshError>)>>`
    - Verify all peers, return results (don't fail on first error)
- [ ] Integration tests (using mockito or wiremock for HTTP mocking):
  - Mock peer endpoint returning valid identity document — verification succeeds
  - Mock peer endpoint returning identity with wrong node_id — verification fails
  - Mock peer endpoint returning tampered identity (bad signature) — verification fails
  - Mock peer endpoint returning 500 — NetworkError
  - Mock peer endpoint timeout — NetworkError

**Key Decisions**:
- Use `reqwest` with reasonable timeouts (10s connect, 30s total)
- Verification is explicit (called by operator or on peer add) — not automatic background task (that's health checks)
- Identity mismatch is a serious error — could indicate MITM or misconfiguration

**Files to Modify**:
- `src/peer.rs`

**Success Criteria**:
- [ ] Valid peer identity verification succeeds
- [ ] Node ID mismatch detected
- [ ] Tampered identity (bad self-signature) detected
- [ ] Network errors handled gracefully
- [ ] `cargo test peer::verify` passes

**Verification**:
```bash
cargo test peer -- --nocapture
```

---

**Completion Notes**:
- **Implementation**: _(describe what was done)_
- **Files Modified**: _(list)_
- **Tests**: _(X tests passing)_
- **Commit**: `feat(peer): peer identity verification via HTTP`

---

### Subtask 5.1.4: Periodic Health Checks (Single Session)

**Prerequisites**:
- [x] 5.1.3: Peer identity verification

**Deliverables**:
- [ ] Implement in `src/peer.rs`:
  - `PeerManager::health_check(&self, node_id: &str) -> MeshResult<bool>`
    - Fetch peer's identity endpoint
    - If reachable and identity valid: update `last_seen`, return true
    - If unreachable: return false (don't remove peer — might be temporary)
  - `PeerManager::spawn_health_check_loop(self: Arc<Self>, interval: Duration) -> tokio::task::JoinHandle<()>`
    - Spawns a tokio task that periodically checks all peers
    - Default interval: 5 minutes
    - Logs results via `tracing` (info for healthy, warn for unreachable)
    - Does NOT remove unhealthy peers — that's the operator's decision
- [ ] Tests:
  - Health check on reachable peer — returns true, updates last_seen
  - Health check on unreachable peer — returns false, no crash
  - Health check loop can be spawned and cancelled

**Key Decisions**:
- Health checks are informational only — they update `last_seen` but don't auto-remove peers
- 5-minute default interval is reasonable for a low-traffic federation
- Use `tokio::select!` with a cancellation token for clean shutdown

**Files to Modify**:
- `src/peer.rs`

**Success Criteria**:
- [ ] Health check updates last_seen for reachable peers
- [ ] Unreachable peers don't cause errors
- [ ] Background loop can be started and stopped
- [ ] `cargo test peer::health` passes

**Verification**:
```bash
cargo test peer -- --nocapture
```

---

**Completion Notes**:
- **Implementation**: _(describe what was done)_
- **Files Modified**: _(list)_
- **Tests**: _(X tests passing)_
- **Commit**: `feat(peer): periodic health checks with background task`

---

## Phase 5 Complete — Squash Merge

- [ ] All subtasks complete (5.1.1 – 5.1.4)
- [ ] `cargo test` — all tests pass
- [ ] `cargo clippy -- -D warnings` — no warnings
- [ ] `cargo fmt --check` — no formatting issues
- [ ] Squash merge to main:
  ```bash
  git checkout main && git merge --squash feature/5-peers
  git commit -m "feat: phase 5 — peer management and binary trust"
  git push origin main
  git branch -d feature/5-peers
  ```

---

*Previous: [Phase 4 — HTTP Endpoints & Wire Protocol](PHASE_4.md) | Next: [Phase 6 — Publication & Revocation](PHASE_6.md)*
