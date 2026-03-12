# Phase 1: Foundation & Core Types

**Goal**: Set up the Rust crate with all dependencies and define core types used throughout the codebase.
**Duration**: 1-2 days
**Branch**: `feature/1-foundation`

## Prerequisites

- None (first phase)

## Context

mesh-node is a library crate that will be consumed by nellie-rs. It needs to expose types and functions that nellie-rs can integrate. The crate is in its own repository (`mesh-rs`) and will be added as a dependency to nellie-rs.

Key dependencies:
- `ed25519-dalek` — Ed25519 signing/verification
- `serde` / `serde_json` — Serialization (already in nellie-rs, must be compatible)
- `axum` — HTTP framework (already in nellie-rs)
- `rusqlite` — SQLite (already in nellie-rs)
- `reqwest` — HTTP client for outbound peer calls
- `json-canonicalize` — RFC 8785 canonical JSON
- `bs58` — Base58 encoding for node IDs
- `base64` — Key/signature encoding
- `sha2` — SHA-256 fingerprints
- `uuid` — Record/request IDs
- `chrono` — Timestamps
- `tokio` — Async runtime (already in nellie-rs)
- `tracing` — Logging (already in nellie-rs)
- `rand` — RNG for key generation

---

## Task 1.1: Crate Setup

### Subtask 1.1.1: Cargo.toml & Project Structure (Single Session)

**Prerequisites**: None

**Deliverables**:
- [ ] Create `Cargo.toml` with package metadata and all dependencies
- [ ] Create `src/lib.rs` with module declarations (all modules, initially empty)
- [ ] Create stub files for every module in the module tree
- [ ] Update `.gitignore` for Rust project (target/, *.swp, .env, etc.)
- [ ] Verify `cargo check` passes (stubs compile)

**Key Decisions**:
- Library crate (`lib.rs`), not binary — nellie-rs imports mesh-node
- Edition 2021
- All feature flags needed for dependencies (e.g., `ed25519-dalek/rand_core`, `reqwest/json`, `rusqlite/bundled`)
- `tokio` as dev-dependency only (for async tests) — nellie-rs provides the runtime

**Module tree to stub out**:
```
src/
├── lib.rs
├── identity.rs
├── signing.rs
├── types.rs
├── envelope.rs
├── error.rs
├── storage/
│   ├── mod.rs
│   ├── identity.rs
│   ├── peers.rs
│   ├── records.rs
│   └── revocations.rs
├── http/
│   ├── mod.rs
│   ├── identity.rs
│   ├── announce.rs
│   ├── search.rs
│   └── peers.rs
├── peer.rs
├── trust.rs
├── publish.rs
├── revoke.rs
└── search.rs
```

**Success Criteria**:
- [ ] `cargo check` succeeds with no errors
- [ ] `Cargo.toml` has all dependencies listed above
- [ ] All module files exist (can be empty or have minimal stubs)
- [ ] `.gitignore` includes `target/`, `Cargo.lock` (library crate convention), `.env`

**Verification**:
```bash
cargo check 2>&1 | tail -5    # Should show "Finished"
find src -name "*.rs" | wc -l  # Should be >= 17
```

---

**Completion Notes**:
- **Implementation**: _(describe what was done)_
- **Files Created**: _(list with line counts)_
- **Tests**: N/A (setup)
- **Build**: _(cargo check: pass/fail)_
- **Commit**: `chore: initialize mesh-node crate with module skeleton`

---

### Subtask 1.1.2: Core Types (Single Session)

**Prerequisites**:
- [x] 1.1.1: Crate setup

**Deliverables**:
- [ ] Define `Visibility` enum in `src/types.rs`: `Private`, `Unlisted`, `Public` with serde serialization (lowercase strings)
- [ ] Define `Publication` struct: `visibility: Visibility`, `published_at: i64` (Unix ms), `topics: Option<Vec<String>>`
- [ ] Define `SignatureBlock` struct: `algorithm: String` (always "ed25519"), `node_id: String`, `public_key: String` (base64), `timestamp: i64`, `sig: String` (base64)
- [ ] Define `SignedLesson` struct: `lesson: serde_json::Value`, `publication: Publication`, `signature: SignatureBlock`
- [ ] Define `SignedCheckpoint` struct: `checkpoint: serde_json::Value`, `publication: Publication`, `signature: SignatureBlock`
- [ ] Define `RecordType` enum: `Lesson`, `Checkpoint` with serde (lowercase)
- [ ] Define `PeerConnection` struct: `node_id: String`, `endpoint: String`, `trust_level: TrustLevel`, `last_seen: Option<i64>`, `connected_since: Option<i64>`
- [ ] Define `TrustLevel` enum: `Full`, `None` (binary for MVP, extensible later)
- [ ] Define `Revocation` struct per spec section 4.1
- [ ] All types derive `Debug, Clone, Serialize, Deserialize` as appropriate
- [ ] Unit tests for serde round-trip on all types
- [ ] Re-export all public types from `lib.rs`

**Design Notes**:
- `lesson` and `checkpoint` fields are `serde_json::Value` (not strongly typed) because MESH must preserve unknown fields per the spec's field extensibility rule (Section 2.5). The signing process operates on the JSON value, not a Rust struct.
- `Visibility` default is `Private` (implement `Default` trait)

**Success Criteria**:
- [ ] All types compile and have correct serde serialization
- [ ] `Visibility::default()` returns `Private`
- [ ] `cargo test` passes for serde round-trip tests
- [ ] Types are re-exported from `lib.rs`

**Verification**:
```bash
cargo test types -- --nocapture   # Should show all type tests passing
cargo doc --no-deps               # Should generate docs for all public types
```

---

**Completion Notes**:
- **Implementation**: _(describe what was done)_
- **Files Created/Modified**: _(list)_
- **Tests**: _(X tests passing)_
- **Build**: _(cargo test: pass/fail)_
- **Commit**: `feat(types): define core MESH types (Visibility, SignedLesson, SignedCheckpoint, etc.)`

---

### Subtask 1.1.3: Error Types (Single Session)

**Prerequisites**:
- [x] 1.1.2: Core types

**Deliverables**:
- [ ] Define `MeshError` enum in `src/error.rs` with variants:
  - `InvalidSignature` — signature verification failed
  - `UntrustedNode(String)` — no trust path to node ID
  - `UnknownRecord(String)` — record ID not found
  - `AlreadyRevoked(String)` — record already revoked
  - `RateLimited` — too many requests
  - `InvalidRequest(String)` — malformed request with detail
  - `StorageError(String)` — database error
  - `NetworkError(String)` — HTTP/connectivity error
  - `IdentityError(String)` — keypair/identity issues
  - `SerializationError(String)` — JSON/canonical serialization error
- [ ] Implement `std::fmt::Display` for `MeshError`
- [ ] Implement `std::error::Error` for `MeshError`
- [ ] Implement `From<rusqlite::Error>` for `MeshError`
- [ ] Implement `From<serde_json::Error>` for `MeshError`
- [ ] Implement `From<reqwest::Error>` for `MeshError`
- [ ] Define `MeshResult<T> = Result<T, MeshError>` type alias
- [ ] Implement `IntoResponse` for `MeshError` (axum: returns JSON error body with appropriate HTTP status code)
- [ ] Define `ErrorResponse` struct matching spec section 9.1: `error: true`, `code: String`, `message: String`, `details: Option<serde_json::Value>`
- [ ] Map each variant to its spec error code string (e.g., `InvalidSignature` → `"INVALID_SIGNATURE"`)
- [ ] Map each variant to HTTP status code (e.g., `InvalidSignature` → 401, `RateLimited` → 429, `UnknownRecord` → 404)
- [ ] Unit tests for Display, error code mapping, and HTTP status mapping

**Success Criteria**:
- [ ] `MeshError` compiles and all From impls work
- [ ] `MeshError::InvalidSignature.to_string()` returns a human-readable message
- [ ] Axum `IntoResponse` produces correct JSON error bodies
- [ ] `cargo test error` passes

**Verification**:
```bash
cargo test error -- --nocapture
```

---

**Completion Notes**:
- **Implementation**: _(describe what was done)_
- **Files Created/Modified**: _(list)_
- **Tests**: _(X tests passing)_
- **Build**: _(cargo test: pass/fail)_
- **Commit**: `feat(error): define MeshError enum with spec error codes and axum integration`

---

## Phase 1 Complete — Squash Merge

- [ ] All subtasks complete (1.1.1 – 1.1.3)
- [ ] `cargo check` passes
- [ ] `cargo test` passes
- [ ] `cargo clippy -- -D warnings` passes
- [ ] `cargo fmt --check` reports no changes
- [ ] Squash merge to main:
  ```bash
  git checkout main && git merge --squash feature/1-foundation
  git commit -m "feat: phase 1 — crate foundation and core types"
  git push origin main
  git branch -d feature/1-foundation
  ```

---

*Next: [Phase 2 — Cryptographic Identity & Signing](PHASE_2.md)*
