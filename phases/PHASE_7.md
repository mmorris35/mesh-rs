# Phase 7: Federated Search

**Goal**: Implement federated search — querying peers for records and merging results with local data.
**Duration**: 2 days
**Branch**: `feature/7-search`

## Prerequisites

- Phase 4 complete (search endpoint handler)
- Phase 5 complete (peer manager)
- Phase 3 complete (remote records storage)

## Context

MESH MVP federated search is single-hop (spec §5.2, simplified for N=2):
1. Agent calls `mesh_search` with a query
2. mesh-node searches local records (own public + cached peer records)
3. mesh-node sends `SearchRequest` to each trusted peer
4. Peers respond with their local matches
5. mesh-node merges all results, ranks by `relevance * trustScore * freshness`
6. Returns merged, ranked results to the agent

Multi-hop search (TTL, requestId deduplication, gossip) is v2.

---

## Task 7.1: Federated Search Implementation

### Subtask 7.1.1: Send Search Requests to Peers (Single Session)

**Prerequisites**:
- [x] 4.1.1: SearchRequest/SearchResponse types
- [x] 5.1.1: PeerManager

**Deliverables**:
- [ ] Implement in `src/search.rs`:
  - `FederatedSearch::new(identity: Arc<NodeIdentity>, peer_manager: Arc<PeerManager>, db: Arc<Mutex<Connection>>, http_client: reqwest::Client) -> Self`
  - `FederatedSearch::search_peer(&self, peer: &PeerConnection, request: &SearchRequest) -> MeshResult<SearchResponse>`
    - Build MeshEnvelope wrapping the SearchRequest
    - POST to `{peer.endpoint}/mesh/v1/search`
    - Parse response as MeshEnvelope containing SearchResponse
    - Set timeout: 10s (don't wait forever for a slow peer)
    - On timeout/error: return empty SearchResponse (don't fail the whole search)
  - `FederatedSearch::search_all_peers(&self, request: &SearchRequest) -> Vec<(String, SearchResponse)>`
    - Search all trusted peers concurrently using `tokio::join!` or `futures::join_all`
    - Collect (node_id, response) pairs
    - Log failures but don't propagate — partial results are better than no results
- [ ] Unit tests:
  - Build SearchRequest, verify JSON structure
  - Verify request includes request_id and origin
- [ ] Integration tests (with HTTP mocking):
  - Mock peer returning search results — parsed correctly
  - Mock peer returning empty results — handled
  - Mock peer timeout — returns empty, doesn't fail
  - Mock peer error — returns empty, doesn't fail
  - Search multiple peers concurrently

**Key Decisions**:
- Concurrent peer search using tokio — don't query peers sequentially
- 10s timeout per peer — prevents slow peers from blocking the whole search
- Peer failures produce empty results, not errors — the user still gets local + other peer results
- request_id is a UUID generated for each search to enable dedup (important for v2 multi-hop, but generated now for protocol compliance)

**Files to Modify**:
- `src/search.rs`
- `src/lib.rs` — re-export

**Success Criteria**:
- [ ] Peer search sends correctly formatted requests
- [ ] Responses are parsed correctly
- [ ] Timeouts and errors are handled gracefully
- [ ] Concurrent peer search works
- [ ] `cargo test search` passes

**Verification**:
```bash
cargo test search -- --nocapture
```

---

**Completion Notes**:
- **Implementation**: _(describe what was done)_
- **Files Modified**: _(list)_
- **Tests**: _(X tests passing)_
- **Commit**: `feat(search): send federated search requests to peers`

---

### Subtask 7.1.2: Process Incoming Search Requests (Single Session)

**Prerequisites**:
- [x] 4.2.3: Search endpoint (stub handler)
- [x] 3.1.4: Remote records storage (search_remote_records)

**Deliverables**:
- [ ] Enhance the search endpoint handler in `src/http/search.rs` (replace stub):
  - Parse incoming MeshEnvelope, extract SearchRequest
  - Validate request:
    - limit clamped to 1-100 (default 20)
    - sender is a known peer
  - Query local records:
    - Own published records (public visibility from local lessons/checkpoints tables where visibility='public')
    - Cached remote records (from mesh_remote_records)
  - Apply filters:
    - record_types filter (lesson, checkpoint, or both)
    - Text search on record content
  - Score results:
    - Basic text relevance score (0.0-1.0) — word match ratio for MVP
    - trust_score: 1.0 (direct, single-hop)
  - Build SearchResponse:
    - Sort by score descending
    - Truncate to limit
    - Set `truncated` flag if more results exist
  - Wrap response in MeshEnvelope and return
- [ ] Integration tests:
  - Store some remote records, search — results returned
  - Search with type filter — only matching types
  - Search with limit — results capped
  - Search empty database — empty results
  - Verify results are sorted by score

**Key Decisions**:
- MVP scoring is simple text matching — not semantic search. nellie-rs has vector search but mesh-node is a library crate and may not have access to the embedding model.
- The search handler queries both own public records AND cached peer records — this enables transitive discovery even in N=2 (if node A has records from node B, and node C searches node A, node C can discover B's records)
- For own records: need to query nellie-rs's lessons/checkpoints tables where visibility='public'. This requires mesh-node to know about those tables.

**Files to Modify**:
- `src/http/search.rs`

**Success Criteria**:
- [ ] Search endpoint returns correctly formatted SearchResponse
- [ ] Filters work (record_type, limit)
- [ ] Results are scored and sorted
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
- **Commit**: `feat(search): process incoming search requests with scoring`

---

### Subtask 7.1.3: Result Merging & Ranking (Single Session)

**Prerequisites**:
- [x] 7.1.1: Peer search
- [x] 7.1.2: Incoming search processing

**Deliverables**:
- [ ] Implement in `src/search.rs`:
  - `FederatedSearch::search(&self, query: &str, record_types: Option<Vec<RecordType>>, filters: Option<SearchFilters>, limit: Option<usize>) -> MeshResult<SearchResponse>`
    - This is the main entry point called by the MCP tool
    1. Generate request_id (UUID)
    2. Build SearchRequest
    3. Query local records (own public + cached remote)
    4. Query all trusted peers concurrently via `search_all_peers()`
    5. Merge all results into a single list
    6. Deduplicate by record ID (keep highest-scored version)
    7. Rank by final score: `relevance * trust_score * freshness_factor`
    8. Apply limit (default 20, max 100)
    9. Return merged SearchResponse
  - `calculate_freshness(published_at: i64) -> f64`
    - Decay function based on age
    - Records from last 24h: 1.0
    - Records from last week: 0.8
    - Records from last month: 0.6
    - Older: 0.4
    - This is a simple step function for MVP — exponential decay is v2
  - `merge_results(local: Vec<SearchResult>, peer_results: Vec<(String, SearchResponse)>) -> Vec<SearchResult>`
    - Combine all results
    - Set `via` field to the peer node_id for peer results
    - Deduplicate by record ID (prefer higher score)
    - Sort by final_score descending
  - Result deduplication: when same record appears from multiple sources, keep the one with the highest combined score
- [ ] Unit tests:
  - Merge local and peer results — combined correctly
  - Deduplication works (same record from two sources → one result)
  - Ranking order is correct (higher score first)
  - Freshness calculation is correct for various ages
  - Limit is applied after merge
  - Empty peer results don't affect local results

**Key Decisions**:
- Final score = `relevance * trust_score * freshness` (spec §5.4)
- For MVP (N=2, single-hop): trust_score is always 1.0 (direct trust)
- Freshness is a simple step function — easy to understand and predictable
- Dedup by record ID keeps highest-scored version

**Files to Modify**:
- `src/search.rs`

**Success Criteria**:
- [ ] Full federated search flow works end-to-end (local + peer)
- [ ] Results are correctly merged, deduped, and ranked
- [ ] Freshness calculation produces expected values
- [ ] Limit is enforced on merged results
- [ ] `cargo test search` passes

**Verification**:
```bash
cargo test search -- --nocapture
```

---

**Completion Notes**:
- **Implementation**: _(describe what was done)_
- **Files Modified**: _(list)_
- **Tests**: _(X tests passing)_
- **Commit**: `feat(search): result merging, deduplication, and ranking`

---

## Phase 7 Complete — Squash Merge

- [ ] All subtasks complete (7.1.1 – 7.1.3)
- [ ] `cargo test` — all tests pass
- [ ] `cargo clippy -- -D warnings` — no warnings
- [ ] `cargo fmt --check` — no formatting issues
- [ ] Squash merge to main:
  ```bash
  git checkout main && git merge --squash feature/7-search
  git commit -m "feat: phase 7 — federated search with result merging"
  git push origin main
  git branch -d feature/7-search
  ```

---

*Previous: [Phase 6 — Publication & Revocation](PHASE_6.md) | Next: [Phase 8 — MCP Tools & Integration](PHASE_8.md)*
