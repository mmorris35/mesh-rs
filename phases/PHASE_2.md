# Phase 2: Cryptographic Identity & Signing

**Goal**: Implement Ed25519 identity management and RFC 8785 canonical JSON signing/verification — the cryptographic foundation of MESH.
**Duration**: 3-4 days
**Branch**: `feature/2-crypto`

## Prerequisites

- Phase 1 complete (core types, error types defined)

## Context

Every MESH node has a long-term Ed25519 keypair. The public key (base58-encoded) serves as the node ID. All shared records are signed using canonical JSON serialization (RFC 8785) to ensure deterministic signatures. The identity document is self-signed to prove key possession.

Key spec references:
- Specification §1: Node Identity
- Specification §2.2: Signing Process
- Specification §2.3: Verification Process
- Specification §2.5: Field Extensibility
- Security Model: Cryptographic Primitives

---

## Task 2.1: Node Identity

### Subtask 2.1.1: Ed25519 Keypair Generation & Node ID (Single Session)

**Prerequisites**:
- [x] 1.1.3: Error types

**Deliverables**:
- [ ] Implement in `src/identity.rs`:
  - `NodeIdentity` struct: holds `SigningKey` (private), `VerifyingKey` (public), derived `node_id` and `fingerprint`
  - `NodeIdentity::generate()` — generate new random Ed25519 keypair using `ed25519_dalek::SigningKey::generate(&mut OsRng)`
  - `NodeIdentity::from_private_key_bytes(bytes: &[u8; 32])` — reconstruct from stored private key
  - `NodeIdentity::node_id(&self) -> &str` — base58-encoded public key
  - `NodeIdentity::fingerprint(&self) -> String` — `sha256(public_key).hex()[:16]`
  - `NodeIdentity::public_key_base64(&self) -> String` — base64-encoded public key (for wire format)
  - `NodeIdentity::private_key_bytes(&self) -> &[u8; 32]` — for secure storage (NEVER log this)
- [ ] Private key must NEVER appear in `Debug` output — implement custom `Debug` that redacts the private key
- [ ] Unit tests:
  - Generate keypair, verify node_id is valid base58
  - Round-trip: generate → export private key bytes → reconstruct → same node_id
  - Fingerprint is 16 hex chars
  - Debug output does not contain private key bytes

**Key Decisions**:
- Use `ed25519_dalek` v2.x with `rand_core` feature for `OsRng`
- Node ID = `bs58::encode(public_key_bytes).into_string()` (matches spec: base58 of public key)
- Fingerprint = first 16 hex chars of SHA-256 of public key bytes (matches spec §1.1)

**Files to Modify**:
- `src/identity.rs` — main implementation
- `src/lib.rs` — re-export `NodeIdentity`

**Success Criteria**:
- [ ] `NodeIdentity::generate()` produces unique keypairs
- [ ] Node ID is a valid base58 string
- [ ] Private key is redacted in Debug output
- [ ] `cargo test identity` passes all tests

**Verification**:
```bash
cargo test identity -- --nocapture
```

---

**Completion Notes**:
- **Implementation**: _(describe what was done)_
- **Files Modified**: _(list)_
- **Tests**: _(X tests passing)_
- **Commit**: `feat(identity): Ed25519 keypair generation and base58 node ID`

---

### Subtask 2.1.2: Identity Document & Self-Signing (Single Session)

**Prerequisites**:
- [x] 2.1.1: Ed25519 keypair generation

**Deliverables**:
- [ ] Define `IdentityDocument` struct in `src/identity.rs`:
  - `version: String` — always `"mesh/1.0"`
  - `node_id: String` — base58 public key
  - `public_key: String` — base64 Ed25519 public key
  - `endpoints: IdentityEndpoints` — struct with `mesh: String` (the `/mesh` base URL)
  - `capabilities: Vec<String>` — `["search"]` for MVP (no sync/directory yet)
  - `signature: String` — base64 self-signature
- [ ] `NodeIdentity::identity_document(&self, mesh_endpoint: &str) -> IdentityDocument`
  - Constructs the identity document
  - Self-signs it: sign the canonical JSON of the document (excluding the signature field) with the node's private key
  - Attaches the signature
- [ ] `IdentityDocument::verify(&self) -> MeshResult<()>`
  - Verifies the self-signature
  - Confirms `node_id` matches base58(public_key)
- [ ] All fields have serde serialization with `camelCase` rename (matching spec JSON format)
- [ ] Unit tests:
  - Create identity document, verify self-signature passes
  - Tamper with a field, verify self-signature fails
  - Verify node_id matches public_key
  - Serde round-trip produces identical JSON

**Key Decisions**:
- The `endpoints.mesh` field is the base URL (e.g., `https://node.example.com/mesh`). The identity endpoint itself is at `/mesh/v1/identity`.
- Self-signing process: serialize document without signature field → canonical JSON → sign → attach signature
- Use `#[serde(rename_all = "camelCase")]` on structs for wire format compatibility

**Files to Modify**:
- `src/identity.rs`

**Success Criteria**:
- [ ] Identity document round-trips through JSON correctly
- [ ] Self-signature verification succeeds for valid documents
- [ ] Self-signature verification fails for tampered documents
- [ ] `cargo test identity` passes

**Verification**:
```bash
cargo test identity -- --nocapture
```

---

**Completion Notes**:
- **Implementation**: _(describe what was done)_
- **Files Modified**: _(list)_
- **Tests**: _(X tests passing)_
- **Commit**: `feat(identity): identity document with self-signing and verification`

---

## Task 2.2: Record Signing

### Subtask 2.2.1: RFC 8785 Canonical JSON (Single Session)

**Prerequisites**:
- [x] 2.1.1: Ed25519 keypair generation

**Deliverables**:
- [ ] Implement canonical JSON helper in `src/signing.rs`:
  - `canonicalize(value: &serde_json::Value) -> MeshResult<String>` — produces RFC 8785 canonical JSON
  - Use the `json-canonicalize` crate
- [ ] Unit tests:
  - Object keys are sorted lexicographically
  - No whitespace in output
  - Numbers in shortest form
  - Nested objects are recursively canonicalized
  - Test with a complex nested structure (simulating a lesson with graph metadata)
  - Verify determinism: same input always produces same output

**Key Decisions**:
- RFC 8785 is critical for signature determinism — any divergence from canonical form will break cross-node verification
- Use `json-canonicalize` crate rather than implementing from scratch
- All fields present in the record (including unknown/extension fields) are included in canonical JSON per spec §2.5

**Files to Modify**:
- `src/signing.rs`

**Success Criteria**:
- [ ] Canonical JSON output matches RFC 8785 rules
- [ ] Deterministic: same input → same output across calls
- [ ] `cargo test signing` passes

**Verification**:
```bash
cargo test signing -- --nocapture
```

---

**Completion Notes**:
- **Implementation**: _(describe what was done)_
- **Files Modified**: _(list)_
- **Tests**: _(X tests passing)_
- **Commit**: `feat(signing): RFC 8785 canonical JSON serialization`

---

### Subtask 2.2.2: Record Signing & Verification (Single Session)

**Prerequisites**:
- [x] 2.2.1: RFC 8785 canonical JSON
- [x] 2.1.1: Ed25519 keypair generation
- [x] 1.1.2: Core types (SignedLesson, SignedCheckpoint, SignatureBlock)

**Deliverables**:
- [ ] Implement in `src/signing.rs`:
  - `sign_lesson(identity: &NodeIdentity, lesson: &serde_json::Value, publication: &Publication) -> MeshResult<SignedLesson>`
    1. Construct signable payload: `{ "lesson": lesson, "publication": publication, "timestamp": now_ms }`
    2. Canonicalize with RFC 8785
    3. Sign canonical bytes with Ed25519
    4. Return `SignedLesson` with attached `SignatureBlock`
  - `sign_checkpoint(identity: &NodeIdentity, checkpoint: &serde_json::Value, publication: &Publication) -> MeshResult<SignedCheckpoint>`
    - Same process, substituting `checkpoint` for `lesson`
  - `verify_signed_lesson(signed: &SignedLesson) -> MeshResult<()>`
    1. Extract lesson, publication, and signature.timestamp
    2. Reconstruct signable payload
    3. Canonicalize
    4. Verify Ed25519 signature against canonical bytes using signature.public_key
    5. Verify node_id == base58(public_key)
    6. Return Ok(()) or Err(MeshError::InvalidSignature)
  - `verify_signed_checkpoint(signed: &SignedCheckpoint) -> MeshResult<()>`
    - Same verification process for checkpoints
  - `sign_revocation(identity: &NodeIdentity, revocation: &Revocation) -> MeshResult<Revocation>`
    - Signs a revocation record
  - `verify_revocation(revocation: &Revocation) -> MeshResult<()>`
    - Verifies revocation signature
- [ ] All signing functions use `chrono::Utc::now().timestamp_millis()` for timestamps
- [ ] Unit tests:
  - Sign a lesson, verify succeeds
  - Sign a checkpoint, verify succeeds
  - Tamper with lesson content after signing, verify fails
  - Tamper with publication after signing, verify fails
  - Use wrong key to verify, fails
  - Sign and verify revocation
  - Verify node_id / public_key mismatch is caught
  - Sign a lesson with extra unknown fields (graph metadata), verify those fields are covered by signature

**Key Design Notes**:
- The signing payload structure must exactly match spec §2.2: `{ lesson|checkpoint, publication, timestamp }`
- Field extensibility: since `lesson`/`checkpoint` are `serde_json::Value`, any extra fields (like `solved_problem`, `used_tools`) are automatically included in canonical JSON
- Timestamp in the signature block is the signing time, which is also included in the signable payload

**Files to Modify**:
- `src/signing.rs`

**Success Criteria**:
- [ ] Sign/verify round-trip works for both lessons and checkpoints
- [ ] Tampered records fail verification with `MeshError::InvalidSignature`
- [ ] Unknown fields are covered by signature (tampering detected)
- [ ] Node ID / public key mismatch is detected
- [ ] `cargo test signing` passes all tests

**Verification**:
```bash
cargo test signing -- --nocapture
```

---

**Completion Notes**:
- **Implementation**: _(describe what was done)_
- **Files Modified**: _(list)_
- **Tests**: _(X tests passing)_
- **Commit**: `feat(signing): Ed25519 record signing and verification for lessons and checkpoints`

---

## Phase 2 Complete — Squash Merge

- [ ] All subtasks complete (2.1.1 – 2.2.2)
- [ ] `cargo test` — all tests pass
- [ ] `cargo clippy -- -D warnings` — no warnings
- [ ] `cargo fmt --check` — no formatting issues
- [ ] Squash merge to main:
  ```bash
  git checkout main && git merge --squash feature/2-crypto
  git commit -m "feat: phase 2 — cryptographic identity and record signing"
  git push origin main
  git branch -d feature/2-crypto
  ```

---

*Previous: [Phase 1 — Foundation & Core Types](PHASE_1.md) | Next: [Phase 3 — Storage Layer](PHASE_3.md)*
