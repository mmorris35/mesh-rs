# mesh-node Development Plan

> Rust crate adding MESH federation to nellie-rs

## Overview

**mesh-node** enables two nellie-rs instances on separate Tailscale tailnets to securely share lessons and checkpoints over the public internet via Cloudflare Tunnel. It implements the [MESH protocol specification](docs/mesh-protocol/SPECIFICATION.md).

- **Type**: Library crate (integrated into nellie-rs binary)
- **Timeline**: 4 weeks
- **Team**: 1
- **Spec**: [SPECIFICATION.md](docs/mesh-protocol/SPECIFICATION.md) | [SECURITY.md](docs/mesh-protocol/SECURITY.md)

## Architecture

```
┌──────────────────────────────────────────────────┐
│                    nellie-rs                      │
│                                                  │
│  ┌────────────────────────────────────────────┐  │
│  │               mesh-node crate              │  │
│  │                                            │  │
│  │  ┌──────────┐ ┌──────────┐ ┌────────────┐ │  │
│  │  │ identity │ │ signing  │ │   types    │ │  │
│  │  │ Ed25519  │ │ RFC 8785 │ │            │ │  │
│  │  └──────────┘ └──────────┘ └────────────┘ │  │
│  │  ┌──────────┐ ┌──────────┐ ┌────────────┐ │  │
│  │  │ storage  │ │   http   │ │   peer     │ │  │
│  │  │ SQLite   │ │   axum   │ │   mgmt     │ │  │
│  │  └──────────┘ └──────────┘ └────────────┘ │  │
│  │  ┌──────────┐ ┌──────────┐ ┌────────────┐ │  │
│  │  │ publish  │ │  revoke  │ │   search   │ │  │
│  │  └──────────┘ └──────────┘ └────────────┘ │  │
│  └────────────────────────────────────────────┘  │
│                                                  │
│  ┌────────────────────────────────────────────┐  │
│  │            MCP Tools (6 new)               │  │
│  │  mesh_publish  mesh_search  mesh_peers     │  │
│  │  mesh_trust    mesh_revoke  mesh_status    │  │
│  └────────────────────────────────────────────┘  │
└──────────────────────────────────────────────────┘
```

## Module Structure

```
src/
├── lib.rs              # MeshNode struct, public API, re-exports
├── identity.rs         # Ed25519 keypair, base58 node ID, identity document
├── signing.rs          # RFC 8785 canonical JSON, sign/verify
├── types.rs            # SignedLesson, SignedCheckpoint, Visibility, Publication, etc.
├── envelope.rs         # MeshEnvelope wire format
├── error.rs            # MeshError enum, error codes
├── storage/
│   ├── mod.rs          # Migrations, MeshStorage struct
│   ├── identity.rs     # mesh_identity table ops
│   ├── peers.rs        # mesh_peers table ops
│   ├── records.rs      # mesh_remote_records table ops
│   └── revocations.rs  # mesh_revocations table ops
├── http/
│   ├── mod.rs          # mesh_router() builder
│   ├── identity.rs     # GET /mesh/v1/identity
│   ├── announce.rs     # POST /mesh/v1/announce
│   ├── search.rs       # POST /mesh/v1/search
│   └── peers.rs        # GET /mesh/v1/peers
├── peer.rs             # PeerManager, health checks
├── trust.rs            # TrustManager (binary for MVP)
├── publish.rs          # Publication flow
├── revoke.rs           # Revocation flow
└── search.rs           # Federated search, result merging
```

## Phase Overview & Progress

| # | Phase | Status | Plan |
|---|-------|--------|------|
| 1 | Foundation & Core Types | ✅ Complete | [phases/PHASE_1.md](phases/PHASE_1.md) |
| 2 | Cryptographic Identity & Signing | ✅ Complete | [phases/PHASE_2.md](phases/PHASE_2.md) |
| 3 | Storage Layer | ✅ Complete | [phases/PHASE_3.md](phases/PHASE_3.md) |
| 4 | HTTP Endpoints & Wire Protocol | ✅ Complete | [phases/PHASE_4.md](phases/PHASE_4.md) |
| 5 | Peer Management & Trust | ✅ Complete | [phases/PHASE_5.md](phases/PHASE_5.md) |
| 6 | Publication & Revocation | ✅ Complete | [phases/PHASE_6.md](phases/PHASE_6.md) |
| 7 | Federated Search | ✅ Complete | [phases/PHASE_7.md](phases/PHASE_7.md) |
| 8 | MCP Tools & Integration | ✅ Complete | [phases/PHASE_8.md](phases/PHASE_8.md) |

### Subtask Checklist

#### Phase 1: Foundation & Core Types
- [x] 1.1.1: Crate setup (Cargo.toml, dependencies, .gitignore)
- [x] 1.1.2: Module skeleton & core types
- [x] 1.1.3: Error types

#### Phase 2: Cryptographic Identity & Signing
- [x] 2.1.1: Ed25519 keypair generation & base58 node ID
- [x] 2.1.2: Identity document struct & self-signing
- [x] 2.2.1: RFC 8785 canonical JSON serialization
- [x] 2.2.2: Record signing & verification (lessons + checkpoints)

#### Phase 3: Storage Layer
- [x] 3.1.1: Schema migrations (visibility columns + new tables)
- [x] 3.1.2: Identity storage (mesh_identity CRUD)
- [x] 3.1.3: Peer & trust storage (mesh_peers CRUD)
- [x] 3.1.4: Remote records & revocations storage

#### Phase 4: HTTP Endpoints & Wire Protocol
- [x] 4.1.1: MeshEnvelope & request/response types
- [x] 4.1.2: Axum router & shared state
- [x] 4.2.1: GET /mesh/v1/identity endpoint
- [x] 4.2.2: POST /mesh/v1/announce endpoint
- [x] 4.2.3: POST /mesh/v1/search endpoint
- [x] 4.2.4: GET /mesh/v1/peers endpoint

#### Phase 5: Peer Management & Trust
- [x] 5.1.1: PeerManager (add/remove/list peers)
- [x] 5.1.2: TrustManager (binary trust for MVP)
- [x] 5.1.3: Peer identity verification
- [x] 5.1.4: Periodic health checks

#### Phase 6: Publication & Revocation
- [x] 6.1.1: Publication flow (sign + announce to peers)
- [x] 6.1.2: Incoming announcement processing
- [x] 6.2.1: Revocation flow (sign + propagate)
- [x] 6.2.2: Incoming revocation processing

#### Phase 7: Federated Search
- [x] 7.1.1: Send search requests to peers
- [x] 7.1.2: Process incoming search requests
- [x] 7.1.3: Result merging & ranking

#### Phase 8: MCP Tools & Integration
- [x] 8.1.1: mesh_status & mesh_peers tools
- [x] 8.1.2: mesh_trust tool
- [x] 8.1.3: mesh_publish tool
- [x] 8.1.4: mesh_revoke tool
- [x] 8.1.5: mesh_search tool
- [x] 8.2.1: MeshNode integration struct
- [x] 8.2.2: End-to-end federation test

## Technology Stack

| Component | Crate | Notes |
|-----------|-------|-------|
| Language | Rust (2021 edition) | |
| HTTP Server | `axum` | Extend nellie-rs's existing server |
| Crypto | `ed25519-dalek` | Ed25519 signing/verification |
| Async | `tokio` | Already in nellie-rs |
| Serialization | `serde`, `serde_json` | Already in nellie-rs |
| Canonical JSON | `json-canonicalize` | RFC 8785 implementation |
| Database | `rusqlite` | Extend nellie-rs's existing DB |
| HTTP Client | `reqwest` | Outbound calls to peers |
| Base58 | `bs58` | Node ID encoding |
| Timestamps | `chrono` | Unix ms timestamps |
| Base64 | `base64` | Key/signature encoding |
| UUID | `uuid` | Record IDs, request IDs |
| RNG | `rand` | Keypair generation |
| Hashing | `sha2` | Fingerprint: SHA-256(pubkey) |
| Logging | `tracing` | Already in nellie-rs |

## Integration with nellie-rs

mesh-node is a library crate that nellie-rs depends on. It exposes:

1. **`MeshNode`** — Main struct holding identity, storage, peer manager, trust manager
2. **`mesh_router(state) -> Router`** — Axum routes to merge into nellie-rs's server
3. **`run_migrations(conn: &Connection)`** — SQLite migrations to run at startup
4. **MCP tool handlers** — Functions nellie-rs registers as MCP tools

nellie-rs integration points:
- Merges mesh routes into its axum router
- Runs mesh migrations on its SQLite connection at startup
- Registers mesh MCP tools alongside existing tools
- Passes its search functionality to mesh for federated search queries

## Git Workflow

### Branch Strategy
- **One branch per phase** (e.g., `feature/1-foundation`)
- Branch when starting first subtask of a phase
- All subtasks within a phase are commits on the phase branch

### Commit Convention
- Format: `feat(scope): description` or `fix(scope): description`
- Types: `feat`, `fix`, `refactor`, `test`, `docs`, `chore`
- Example: `feat(identity): implement Ed25519 keypair generation`

### Merge Strategy
- Squash merge when phase is complete (all subtasks done, tests pass)
- ```bash
  git checkout main && git merge --squash feature/N-name
  git commit -m "feat: phase N - description"
  git push origin main
  git branch -d feature/N-name
  ```

### Per-Subtask Completion
After each subtask:
```bash
# Stage and commit
git add <specific files>
git commit -m "feat(scope): subtask description"

# Push feature branch for backup
git push -u origin feature/N-name

# Verify
cargo test
cargo clippy -- -D warnings
cargo fmt --check
```

## Open Questions

1. **Crate dependency**: How does nellie-rs depend on mesh-node? Git dependency, cargo workspace, or crates.io publish?
2. **Database sharing**: Does mesh-node receive a `rusqlite::Connection` from nellie-rs, or open its own to the same DB file?
3. **Search interface**: How does nellie-rs expose its local search for mesh_search to query local records?
4. **Existing schema**: Need to read nellie-rs's actual `lessons` and `checkpoints` table schemas before writing ALTER TABLE migrations in Phase 3.

## v2 Roadmap (Post-MVP)

After MVP is stable with N=2 federation:

| Feature | Notes |
|---------|-------|
| Web of Trust | Transitive trust, configurable depth (needed when N>2) |
| Multi-hop search | TTL, requestId dedup, gossip propagation |
| Directory server | Register, search, list nodes |
| Bulk sync | Cursor-based resumption, topic/recordType filtering |
| Crypto revocation | AES-256-GCM encrypted records, revoke key = content dies |
| Key escrow | Shamir secret sharing |
| E2E encryption | X25519 + XChaCha20-Poly1305 |
| Consumer/reader tiers | Read-only access without full node identity |
| LoRa/Meshtastic | Off-grid sensor network gateway |
| App-layer TLS 1.3 | For deployments without Cloudflare Tunnel |

---

*Phase details in [phases/](phases/) directory.*
