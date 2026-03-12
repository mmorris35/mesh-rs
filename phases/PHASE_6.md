# Phase 6: Publication & Revocation

**Goal**: Implement the publish and revoke flows — signing records and announcing them to peers, and processing incoming announcements.
**Duration**: 2-3 days
**Branch**: `feature/6-publish`

## Prerequisites

- Phase 2 complete (signing)
- Phase 3 complete (storage — remote records, revocations)
- Phase 4 complete (HTTP announce endpoint)
- Phase 5 complete (peer manager, trust manager)

## Context

Publication flow (spec §3.2):
1. Operator calls `mesh_publish` with a record ID and visibility
2. mesh-node signs the record with the node's Ed25519 key
3. mesh-node wraps the signed record in a MeshEnvelope
4. mesh-node POSTs the envelope to each trusted peer's `/mesh/v1/announce` endpoint

Revocation flow (spec §4.3):
1. Operator calls `mesh_revoke` with a record ID
2. mesh-node signs a revocation record
3. mesh-node wraps the revocation in a MeshEnvelope
4. mesh-node POSTs to each trusted peer's `/mesh/v1/announce`
5. Peers verify the revoker matches the original publisher, then delete cached copy

---

## Task 6.1: Publication

### Subtask 6.1.1: Publication Flow (Single Session)

**Prerequisites**:
- [x] 2.2.2: Record signing
- [x] 5.1.1: PeerManager

**Deliverables**:
- [ ] Implement `Publisher` in `src/publish.rs`:
  - `Publisher::new(identity: Arc<NodeIdentity>, peer_manager: Arc<PeerManager>, http_client: reqwest::Client) -> Self`
  - `Publisher::publish_lesson(&self, lesson: serde_json::Value, visibility: Visibility, topics: Option<Vec<String>>) -> MeshResult<SignedLesson>`
    1. Create `Publication` with visibility, current timestamp, topics
    2. Sign lesson using `sign_lesson()`
    3. Build `PublicationAnnouncement` with record_type=Lesson
    4. Wrap in `MeshEnvelope` (type="publication", signed)
    5. Send to all trusted peers via `announce_to_peers()`
    6. Return the `SignedLesson`
  - `Publisher::publish_checkpoint(&self, checkpoint: serde_json::Value, visibility: Visibility, topics: Option<Vec<String>>) -> MeshResult<SignedCheckpoint>`
    - Same flow for checkpoints
  - `Publisher::announce_to_peers(&self, envelope: &MeshEnvelope) -> MeshResult<Vec<(String, Result<(), MeshError>)>>`
    - POST envelope to `{peer.endpoint}/mesh/v1/announce` for each trusted peer
    - Collect results — don't fail if one peer is unreachable
    - Log successes and failures via `tracing`
    - Return list of (node_id, result) pairs
- [ ] Unit tests:
  - Publish lesson produces valid SignedLesson with correct signature
  - Publish checkpoint produces valid SignedCheckpoint
  - Publication visibility is set correctly
  - Publication timestamp is recent
- [ ] Integration tests (with HTTP mocking):
  - Announce to reachable peer — returns success
  - Announce to unreachable peer — returns error for that peer, doesn't fail overall
  - Announce to multiple peers — results collected per-peer

**Key Decisions**:
- Announce is best-effort: failures to reach individual peers are logged but don't fail the publish operation
- The signed record is returned to the caller so it can also be stored locally (visibility update in nellie-rs)
- `reqwest` client with timeout (10s) for announce calls

**Files to Modify**:
- `src/publish.rs`
- `src/lib.rs` — re-export

**Success Criteria**:
- [ ] Publish produces correctly signed records
- [ ] Announce sends to all trusted peers
- [ ] Partial peer failures don't block the publish
- [ ] `cargo test publish` passes

**Verification**:
```bash
cargo test publish -- --nocapture
```

---

**Completion Notes**:
- **Implementation**: _(describe what was done)_
- **Files Modified**: _(list)_
- **Tests**: _(X tests passing)_
- **Commit**: `feat(publish): publication flow with peer announcement`

---

### Subtask 6.1.2: Incoming Announcement Processing (Single Session)

**Prerequisites**:
- [x] 4.2.2: Announce endpoint (stub handler)
- [x] 3.1.4: Remote records storage
- [x] 5.1.2: TrustManager

**Deliverables**:
- [ ] Enhance the announce endpoint handler in `src/http/announce.rs` (replace stub):
  - For **publication** announcements:
    1. Verify envelope signature
    2. Check sender is a known peer: `peer_manager.is_known_peer(sender)`
    3. Check sender is trusted: `trust_manager.is_trusted(sender)`
    4. Parse payload as `PublicationAnnouncement`
    5. Extract signed record (lesson or checkpoint based on record_type)
    6. Verify the signed record's signature: `verify_signed_lesson()` or `verify_signed_checkpoint()`
    7. Check not already revoked: `storage.is_revoked(record_id)`
    8. Store in mesh_remote_records
    9. Return 200 with `{"accepted": true}`
  - For **revocation** announcements (handled in subtask 6.2.2):
    - Delegate to revocation processing (implemented next)
  - Error responses:
    - Unknown peer → 403 with `UNTRUSTED_NODE`
    - Untrusted peer → 403 with `UNTRUSTED_NODE`
    - Invalid signature → 401 with `INVALID_SIGNATURE`
    - Already revoked → 409 with `ALREADY_REVOKED`
- [ ] Update integration tests from 4.2.2 to test full flow (not stubs)

**Key Decisions**:
- Security order: check known → check trusted → verify signature → check revocation → store
- Both envelope signature AND inner record signature must be verified
- The sender in the envelope must match the signer in the record's signature block

**Files to Modify**:
- `src/http/announce.rs`

**Success Criteria**:
- [ ] Valid publications from trusted peers are accepted
- [ ] Unknown/untrusted peers are rejected (403)
- [ ] Invalid signatures are rejected (401)
- [ ] Already-revoked records are rejected (409)
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
- **Commit**: `feat(announce): full announcement processing with security checks`

---

## Task 6.2: Revocation

### Subtask 6.2.1: Revocation Flow (Single Session)

**Prerequisites**:
- [x] 2.2.2: Record signing (sign_revocation)
- [x] 6.1.1: Publisher (announce_to_peers)

**Deliverables**:
- [ ] Implement in `src/revoke.rs`:
  - `Revoker::new(identity: Arc<NodeIdentity>, peer_manager: Arc<PeerManager>, http_client: reqwest::Client) -> Self`
  - `Revoker::revoke(&self, record_id: &str, record_type: RecordType, reason: Option<&str>) -> MeshResult<Revocation>`
    1. Build `Revocation` struct: record_id, record_type, node_id (self), revoked_at (now), reason
    2. Sign the revocation with `sign_revocation()`
    3. Wrap in `MeshEnvelope` (type="revocation", signed)
    4. Send to all trusted peers via announce (reuse publish's announce logic)
    5. Return the signed `Revocation`
  - Only the original publisher (this node) can revoke — the node_id in the revocation must match the signer
- [ ] Unit tests:
  - Revoke produces valid signed Revocation
  - Revocation node_id matches this node's identity
  - Revocation timestamp is recent
- [ ] Integration tests (with HTTP mocking):
  - Revocation sent to peers successfully

**Files to Modify**:
- `src/revoke.rs`
- `src/lib.rs` — re-export

**Success Criteria**:
- [ ] Revocation is correctly signed
- [ ] Revocation is announced to all trusted peers
- [ ] `cargo test revoke` passes

**Verification**:
```bash
cargo test revoke -- --nocapture
```

---

**Completion Notes**:
- **Implementation**: _(describe what was done)_
- **Files Modified**: _(list)_
- **Tests**: _(X tests passing)_
- **Commit**: `feat(revoke): revocation flow with peer announcement`

---

### Subtask 6.2.2: Incoming Revocation Processing (Single Session)

**Prerequisites**:
- [x] 6.1.2: Announcement processing (announce handler)
- [x] 3.1.4: Revocations storage

**Deliverables**:
- [ ] Enhance the announce handler in `src/http/announce.rs` for revocation type:
  - When envelope type is "revocation":
    1. Verify envelope signature
    2. Check sender is known and trusted
    3. Parse payload as `Revocation`
    4. Verify revocation signature
    5. Verify the revoker's node_id matches the original publisher of the record
       - Look up record in mesh_remote_records — check publisher_node_id
       - If record not cached, still accept revocation (prevents future announcements)
    6. Store revocation in mesh_revocations
    7. Delete cached record from mesh_remote_records (if exists)
    8. Return 200 with `{"accepted": true}`
  - Error responses:
    - Revoker doesn't match publisher → 403
    - Invalid signature → 401
- [ ] Integration tests:
  - Receive revocation for cached record → record deleted, revocation stored
  - Receive revocation for unknown record → revocation stored (prevents future announcements)
  - Receive revocation from non-publisher → 403 rejected
  - After revocation, new announcement of same record → rejected (already revoked)

**Key Decisions**:
- Revocations from non-publishers are rejected (spec §4.2 rule 1)
- Revocations for unknown records are still stored — this prevents race conditions where revocation arrives before the publication
- Revocation records are retained permanently (spec §4.2 rule 5)

**Files to Modify**:
- `src/http/announce.rs`

**Success Criteria**:
- [ ] Revocation deletes cached record
- [ ] Revocation prevents future storage of the record
- [ ] Non-publisher revocations are rejected
- [ ] `cargo test http::announce` passes (including revocation tests)

**Verification**:
```bash
cargo test http::announce -- --nocapture
```

---

**Completion Notes**:
- **Implementation**: _(describe what was done)_
- **Files Modified**: _(list)_
- **Tests**: _(X tests passing)_
- **Commit**: `feat(revoke): incoming revocation processing in announce handler`

---

## Phase 6 Complete — Squash Merge

- [ ] All subtasks complete (6.1.1 – 6.2.2)
- [ ] `cargo test` — all tests pass
- [ ] `cargo clippy -- -D warnings` — no warnings
- [ ] `cargo fmt --check` — no formatting issues
- [ ] Squash merge to main:
  ```bash
  git checkout main && git merge --squash feature/6-publish
  git commit -m "feat: phase 6 — publication and revocation flows"
  git push origin main
  git branch -d feature/6-publish
  ```

---

*Previous: [Phase 5 — Peer Management & Trust](PHASE_5.md) | Next: [Phase 7 — Federated Search](PHASE_7.md)*
