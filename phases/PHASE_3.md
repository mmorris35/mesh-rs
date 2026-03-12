# Phase 3: Storage Layer

**Goal**: Implement SQLite schema migrations and CRUD operations for all mesh-related tables.
**Duration**: 2-3 days
**Branch**: `feature/3-storage`

## Prerequisites

- Phase 1 complete (core types)
- Phase 2 complete (identity types, SignedLesson/SignedCheckpoint)

## Context

mesh-node extends nellie-rs's existing SQLite database. It needs to:
1. Add a `visibility` column to existing `lessons` and `checkpoints` tables (non-destructive migration)
2. Create 4 new tables: `mesh_identity`, `mesh_peers`, `mesh_remote_records`, `mesh_revocations`

All migrations must be idempotent (safe to run multiple times) and non-destructive (existing data preserved, new columns have safe defaults).

**Important**: The actual column names/types in nellie-rs's `lessons` and `checkpoints` tables need to be verified before writing ALTER TABLE statements. The migration should handle the case where the `visibility` column already exists.

---

## Task 3.1: Schema & Migrations

### Subtask 3.1.1: Schema Migrations (Single Session)

**Prerequisites**:
- [x] 1.1.2: Core types (Visibility enum)

**Deliverables**:
- [ ] Implement in `src/storage/mod.rs`:
  - `run_migrations(conn: &rusqlite::Connection) -> MeshResult<()>` — runs all mesh migrations
  - Each migration is idempotent (uses `IF NOT EXISTS`, checks column existence before ALTER)
- [ ] Migration 1 — Add visibility to existing tables:
  ```sql
  ALTER TABLE lessons ADD COLUMN visibility TEXT NOT NULL DEFAULT 'private';
  ALTER TABLE checkpoints ADD COLUMN visibility TEXT NOT NULL DEFAULT 'private';
  ```
  - Must check if column already exists before running ALTER (SQLite doesn't support `IF NOT EXISTS` on ALTER)
  - Use `PRAGMA table_info(lessons)` to check for column existence
- [ ] Migration 2 — mesh_identity table:
  ```sql
  CREATE TABLE IF NOT EXISTS mesh_identity (
    id INTEGER PRIMARY KEY CHECK (id = 1),  -- singleton row
    private_key BLOB NOT NULL,              -- 32 bytes Ed25519 private key
    public_key BLOB NOT NULL,               -- 32 bytes Ed25519 public key
    node_id TEXT NOT NULL,                   -- base58 encoded public key
    fingerprint TEXT NOT NULL,               -- SHA-256(public_key) first 16 hex chars
    created_at INTEGER NOT NULL              -- Unix ms
  );
  ```
- [ ] Migration 3 — mesh_peers table:
  ```sql
  CREATE TABLE IF NOT EXISTS mesh_peers (
    node_id TEXT PRIMARY KEY,               -- peer's base58 node ID
    endpoint TEXT NOT NULL,                  -- peer's tunnel URL
    trust_level TEXT NOT NULL DEFAULT 'none', -- 'full' or 'none'
    last_seen INTEGER,                       -- Unix ms, NULL if never contacted
    connected_since INTEGER,                 -- Unix ms, NULL if never connected
    created_at INTEGER NOT NULL              -- Unix ms when peer was added
  );
  ```
- [ ] Migration 4 — mesh_remote_records table:
  ```sql
  CREATE TABLE IF NOT EXISTS mesh_remote_records (
    id TEXT PRIMARY KEY,                     -- record ID (lesson or checkpoint ID)
    record_type TEXT NOT NULL,               -- 'lesson' or 'checkpoint'
    publisher_node_id TEXT NOT NULL,         -- node ID of original publisher
    signed_record TEXT NOT NULL,             -- full signed record as JSON
    visibility TEXT NOT NULL,                -- 'unlisted' or 'public'
    received_at INTEGER NOT NULL,            -- Unix ms when we received it
    FOREIGN KEY (publisher_node_id) REFERENCES mesh_peers(node_id)
  );
  ```
- [ ] Migration 5 — mesh_revocations table:
  ```sql
  CREATE TABLE IF NOT EXISTS mesh_revocations (
    record_id TEXT PRIMARY KEY,             -- ID of the revoked record
    record_type TEXT NOT NULL,              -- 'lesson' or 'checkpoint'
    publisher_node_id TEXT NOT NULL,        -- who published & revoked
    revocation_json TEXT NOT NULL,          -- full revocation record as JSON
    revoked_at INTEGER NOT NULL,            -- Unix ms
    received_at INTEGER NOT NULL            -- Unix ms when we received the revocation
  );
  ```
- [ ] Create index on `mesh_remote_records(publisher_node_id)`
- [ ] Create index on `mesh_remote_records(record_type)`
- [ ] Unit tests with in-memory SQLite:
  - Run migrations on fresh DB — all tables created
  - Run migrations twice — idempotent, no errors
  - Run migrations on DB that already has lessons/checkpoints tables — visibility column added
  - Verify column existence check works correctly

**Key Decisions**:
- `mesh_identity` uses `CHECK (id = 1)` to enforce singleton — only one identity per node
- `mesh_remote_records.signed_record` stores the complete JSON of the `SignedLesson` or `SignedCheckpoint` — this preserves all fields including unknown ones (per spec §2.5 field extensibility)
- Foreign key on `mesh_remote_records.publisher_node_id` references `mesh_peers` — you can only store records from known peers
- Revocations are retained permanently (per spec §4.2 rule 5) to reject re-announcements of revoked records

**Files to Create/Modify**:
- `src/storage/mod.rs` — migration runner and `MeshStorage` struct

**Success Criteria**:
- [ ] All migrations run without error on fresh in-memory SQLite
- [ ] Migrations are idempotent
- [ ] `PRAGMA table_info` confirms all columns exist after migration
- [ ] `cargo test storage` passes

**Verification**:
```bash
cargo test storage::tests -- --nocapture
```

---

**Completion Notes**:
- **Implementation**: _(describe what was done)_
- **Files Modified**: _(list)_
- **Tests**: _(X tests passing)_
- **Commit**: `feat(storage): SQLite schema migrations for mesh tables`

---

### Subtask 3.1.2: Identity Storage (Single Session)

**Prerequisites**:
- [x] 3.1.1: Schema migrations
- [x] 2.1.1: NodeIdentity struct

**Deliverables**:
- [ ] Implement in `src/storage/identity.rs`:
  - `MeshStorage::save_identity(conn: &Connection, identity: &NodeIdentity) -> MeshResult<()>`
    - INSERT OR REPLACE into mesh_identity (singleton, id=1)
    - Stores private key bytes, public key bytes, node_id, fingerprint, created_at
  - `MeshStorage::load_identity(conn: &Connection) -> MeshResult<Option<NodeIdentity>>`
    - SELECT from mesh_identity WHERE id = 1
    - Reconstruct `NodeIdentity` from stored private key bytes
    - Return `None` if no identity exists yet
  - `MeshStorage::has_identity(conn: &Connection) -> MeshResult<bool>`
    - Quick check if identity exists
- [ ] Unit tests:
  - Save identity, load it back, verify node_id matches
  - Load from empty table returns None
  - has_identity returns false then true after save
  - Save twice overwrites (singleton behavior)

**Files to Modify**:
- `src/storage/identity.rs`
- `src/storage/mod.rs` — re-export

**Success Criteria**:
- [ ] Round-trip: save → load produces identical NodeIdentity (same node_id)
- [ ] `cargo test storage::identity` passes

**Verification**:
```bash
cargo test storage::identity -- --nocapture
```

---

**Completion Notes**:
- **Implementation**: _(describe what was done)_
- **Files Modified**: _(list)_
- **Tests**: _(X tests passing)_
- **Commit**: `feat(storage): identity storage (save/load Ed25519 keypair)`

---

### Subtask 3.1.3: Peer & Trust Storage (Single Session)

**Prerequisites**:
- [x] 3.1.1: Schema migrations

**Deliverables**:
- [ ] Implement in `src/storage/peers.rs`:
  - `MeshStorage::add_peer(conn: &Connection, node_id: &str, endpoint: &str) -> MeshResult<()>`
    - INSERT OR IGNORE into mesh_peers with trust_level='none'
  - `MeshStorage::remove_peer(conn: &Connection, node_id: &str) -> MeshResult<()>`
    - DELETE from mesh_peers
    - Also delete related mesh_remote_records from that peer
  - `MeshStorage::list_peers(conn: &Connection) -> MeshResult<Vec<PeerConnection>>`
    - SELECT all from mesh_peers, map to PeerConnection structs
  - `MeshStorage::get_peer(conn: &Connection, node_id: &str) -> MeshResult<Option<PeerConnection>>`
    - SELECT single peer by node_id
  - `MeshStorage::set_trust(conn: &Connection, node_id: &str, level: TrustLevel) -> MeshResult<()>`
    - UPDATE mesh_peers SET trust_level = ? WHERE node_id = ?
    - Error if peer doesn't exist
  - `MeshStorage::update_last_seen(conn: &Connection, node_id: &str, timestamp: i64) -> MeshResult<()>`
    - UPDATE last_seen and connected_since (if first connection)
  - `MeshStorage::get_trusted_peers(conn: &Connection) -> MeshResult<Vec<PeerConnection>>`
    - SELECT from mesh_peers WHERE trust_level = 'full'
- [ ] Unit tests:
  - Add peer, list includes it
  - Remove peer, list excludes it
  - Remove peer cascades to remote records
  - Set trust, verify it persists
  - Set trust on nonexistent peer returns error
  - Update last_seen
  - get_trusted_peers only returns trusted peers

**Files to Modify**:
- `src/storage/peers.rs`
- `src/storage/mod.rs` — re-export

**Success Criteria**:
- [ ] Full CRUD cycle works for peers
- [ ] Trust level persists correctly
- [ ] Removing peer cleans up related records
- [ ] `cargo test storage::peers` passes

**Verification**:
```bash
cargo test storage::peers -- --nocapture
```

---

**Completion Notes**:
- **Implementation**: _(describe what was done)_
- **Files Modified**: _(list)_
- **Tests**: _(X tests passing)_
- **Commit**: `feat(storage): peer and trust storage CRUD`

---

### Subtask 3.1.4: Remote Records & Revocations Storage (Single Session)

**Prerequisites**:
- [x] 3.1.1: Schema migrations
- [x] 3.1.3: Peer storage (foreign key dependency)

**Deliverables**:
- [ ] Implement in `src/storage/records.rs`:
  - `MeshStorage::store_remote_record(conn, id, record_type, publisher_node_id, signed_record_json, visibility) -> MeshResult<()>`
    - INSERT OR REPLACE into mesh_remote_records
    - Check revocations table first — reject if already revoked
  - `MeshStorage::get_remote_record(conn, id) -> MeshResult<Option<RemoteRecord>>` (define `RemoteRecord` helper struct)
  - `MeshStorage::delete_remote_record(conn, id) -> MeshResult<()>`
  - `MeshStorage::search_remote_records(conn, query: &str, record_type: Option<RecordType>, limit: usize) -> MeshResult<Vec<RemoteRecord>>`
    - Basic text search (LIKE) on signed_record JSON — sufficient for MVP
    - Filter by record_type if specified
  - `MeshStorage::list_remote_records_by_publisher(conn, node_id: &str) -> MeshResult<Vec<RemoteRecord>>`
- [ ] Implement in `src/storage/revocations.rs`:
  - `MeshStorage::store_revocation(conn, record_id, record_type, publisher_node_id, revocation_json) -> MeshResult<()>`
    - INSERT into mesh_revocations
    - Also DELETE from mesh_remote_records if cached copy exists
  - `MeshStorage::is_revoked(conn, record_id) -> MeshResult<bool>`
    - Check mesh_revocations table
  - `MeshStorage::get_revocation(conn, record_id) -> MeshResult<Option<StoredRevocation>>` (define helper struct)
- [ ] Unit tests:
  - Store a remote record, retrieve it
  - Store record, revoke it — record deleted, revocation stored
  - Attempt to store already-revoked record — rejected
  - Search records by text query
  - Search records by type filter
  - Delete record
  - List records by publisher

**Files to Modify**:
- `src/storage/records.rs`
- `src/storage/revocations.rs`
- `src/storage/mod.rs` — re-export

**Success Criteria**:
- [ ] Store/retrieve/delete remote records works
- [ ] Revocation deletes cached record and prevents re-storage
- [ ] Text search returns matching records
- [ ] `cargo test storage::records` and `cargo test storage::revocations` pass

**Verification**:
```bash
cargo test storage -- --nocapture
```

---

**Completion Notes**:
- **Implementation**: _(describe what was done)_
- **Files Modified**: _(list)_
- **Tests**: _(X tests passing)_
- **Commit**: `feat(storage): remote records and revocations storage`

---

## Phase 3 Complete — Squash Merge

- [ ] All subtasks complete (3.1.1 – 3.1.4)
- [ ] `cargo test` — all tests pass
- [ ] `cargo clippy -- -D warnings` — no warnings
- [ ] `cargo fmt --check` — no formatting issues
- [ ] Squash merge to main:
  ```bash
  git checkout main && git merge --squash feature/3-storage
  git commit -m "feat: phase 3 — SQLite storage layer with migrations"
  git push origin main
  git branch -d feature/3-storage
  ```

---

*Previous: [Phase 2 — Cryptographic Identity & Signing](PHASE_2.md) | Next: [Phase 4 — HTTP Endpoints & Wire Protocol](PHASE_4.md)*
