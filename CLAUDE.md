# CLAUDE.md — mesh-node Project Rules

> Read at the start of every session. Defines how to work on this project.

## Project Overview

**mesh-node** is a Rust library crate that adds MESH federation to nellie-rs. It enables two nellie-rs instances on separate Tailscale tailnets to securely share lessons and checkpoints over the public internet via Cloudflare Tunnel.

- **Type**: Library crate (consumed by nellie-rs)
- **Protocol Spec**: `docs/mesh-protocol/SPECIFICATION.md`
- **Security Model**: `docs/mesh-protocol/SECURITY.md`
- **Development Plan**: `DEVELOPMENT_PLAN.md` (overview) + `phases/PHASE_N.md` (details)

## Technology Stack

| Component | Crate/Tool | Notes |
|-----------|-----------|-------|
| Language | Rust (2021 edition) | Stable toolchain |
| HTTP Server | `axum` | Extend nellie-rs's server |
| Crypto | `ed25519-dalek` | Ed25519 signing/verification |
| Async | `tokio` | Runtime provided by nellie-rs |
| Serialization | `serde`, `serde_json` | JSON with `camelCase` for wire format |
| Canonical JSON | `json-canonicalize` | RFC 8785 for deterministic signing |
| Database | `rusqlite` | Extend nellie-rs's SQLite DB |
| HTTP Client | `reqwest` | Outbound peer calls |
| Base58 | `bs58` | Node ID encoding |
| Base64 | `base64` | Key/signature encoding |
| Hashing | `sha2` | SHA-256 fingerprints |
| Logging | `tracing` | Structured logging |

## Build & Test Commands

```bash
# Check compilation
cargo check

# Run all tests
cargo test

# Run tests with output
cargo test -- --nocapture

# Run specific test module
cargo test identity -- --nocapture

# Linting (must pass with no warnings)
cargo clippy -- -D warnings

# Formatting check
cargo fmt --check

# Build documentation
cargo doc --no-deps
```

**All four checks must pass before every commit:**
1. `cargo test` — all tests pass
2. `cargo clippy -- -D warnings` — zero warnings
3. `cargo fmt --check` — no formatting issues
4. `cargo check` — compiles cleanly

## Code Standards

### Rust Conventions
- Follow Rust API Guidelines
- Use `Result<T, MeshError>` (aliased as `MeshResult<T>`) for fallible operations
- Use `///` doc comments on all public items
- Use `#[serde(rename_all = "camelCase")]` on wire format structs
- Private keys must NEVER appear in `Debug` output — use custom `Debug` impls

### Module Organization
- One concept per file (identity, signing, peer, trust, etc.)
- `storage/` submodule for all SQLite operations
- `http/` submodule for all axum endpoint handlers
- Re-export public types from `lib.rs`

### Error Handling
- All errors go through `MeshError` enum in `src/error.rs`
- Use `?` operator, not `.unwrap()` in library code
- `.unwrap()` is acceptable only in tests
- Map external errors via `From` impls

### Security Rules
- **NEVER** log private key material
- **NEVER** transmit private keys
- **ALWAYS** verify signatures before storing remote records
- **ALWAYS** check trust before accepting announcements
- **ALWAYS** use canonical JSON (RFC 8785) for signing

### Testing
- Unit tests in `#[cfg(test)]` module within each source file
- Integration tests in `tests/` directory
- Use in-memory SQLite (`:memory:`) for storage tests
- Use HTTP mocking (wiremock or similar) for peer communication tests
- Target >= 80% test coverage

## Development Plan Structure

The plan is split for manageability:
- **`DEVELOPMENT_PLAN.md`** — Overview, progress checklist, architecture
- **`phases/PHASE_1.md`** through **`phases/PHASE_8.md`** — Detailed subtask plans

When working on subtask X.Y.Z:
1. Check `DEVELOPMENT_PLAN.md` for prerequisite completion
2. Read `phases/PHASE_X.md` for full subtask details
3. Update both files when done

## Git Workflow

### Branch Strategy
- **One branch per phase**: `feature/N-description` (e.g., `feature/1-foundation`)
- Create branch when starting first subtask of a phase
- All subtasks within a phase are commits on the phase branch

### Commit Convention
- Format: `feat(scope): description` or `fix(scope): description`
- Types: `feat`, `fix`, `refactor`, `test`, `docs`, `chore`
- Scope matches module name: `identity`, `signing`, `storage`, `http`, `peer`, `trust`, `publish`, `revoke`, `search`, `tools`

### Per-Subtask Completion
```bash
git add <specific files>
git commit -m "feat(scope): subtask description"
git push -u origin feature/N-description
```

### Phase Complete — Squash Merge
```bash
git checkout main && git merge --squash feature/N-description
git commit -m "feat: phase N — description"
git push origin main
git branch -d feature/N-description
```

## Session Checklist

### Starting a Session
- [ ] Read this file (CLAUDE.md)
- [ ] Read DEVELOPMENT_PLAN.md for progress status
- [ ] Read the relevant `phases/PHASE_X.md` for subtask details
- [ ] Verify prerequisites are complete
- [ ] Check git branch state

### Ending a Session
- [ ] All deliverable checkboxes checked in phase file
- [ ] Subtask checkbox checked in DEVELOPMENT_PLAN.md
- [ ] Completion notes filled in
- [ ] `cargo test` passes
- [ ] `cargo clippy -- -D warnings` passes
- [ ] `cargo fmt --check` passes
- [ ] Git commit with semantic message
- [ ] Branch pushed to remote

## Agents

- **Executor**: `.claude/agents/mesh-node-executor.md` — Haiku-powered, executes subtasks mechanically
  ```
  Use the mesh-node-executor agent to execute subtask X.Y.Z
  ```
- **Verifier**: `.claude/agents/mesh-node-verifier.md` — Sonnet-powered, validates against PROJECT_BRIEF.md
  ```
  Use the mesh-node-verifier agent to validate the application against PROJECT_BRIEF.md
  ```
