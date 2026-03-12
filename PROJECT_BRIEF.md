# PROJECT_BRIEF.md

## Basic Information

- **Project Name**: mesh-node
- **Project Type**: library
- **Primary Goal**: Rust crate that adds MESH federation to nellie-rs, enabling two nellie-rs instances on separate Tailscale networks to securely share lessons and checkpoints over the public internet via Cloudflare Tunnel.
- **Target Users**: AI coding agents that use nellie-rs for persistent memory across machines, nellie-rs operators who want to federate knowledge between nodes
- **Timeline**: 4 weeks
- **Team Size**: 1

## Network Topology

Two nellie-rs instances on **separate Tailscale tailnets** with no sharing configured. Neither node can reach the other via Tailscale. Each node exposes itself to the public internet via Cloudflare Tunnel (free tier, outbound-only). MESH communication happens over these tunnel URLs.

```
Tailnet A                              Tailnet B
┌──────────────┐                       ┌──────────────┐
│  nellie-rs   │                       │  nellie-rs   │
│  + mesh-node │                       │  + mesh-node │
└──────┬───────┘                       └──────┬───────┘
       │ outbound                             │ outbound
       ▼                                      ▼
┌──────────────┐                       ┌──────────────┐
│  cloudflared │                       │  cloudflared │
│  tunnel      │                       │  tunnel      │
└──────┬───────┘                       └──────┬───────┘
       │                                      │
       └──────────► Internet ◄────────────────┘
                  (HTTPS both ways)
```

TLS is handled by Cloudflare Tunnel. The MESH crate does not manage certificates. Ed25519 signatures provide identity and authenticity on top of the tunnel's transport encryption.

## Functional Requirements

### Key Features (MVP)

**Node Identity**
- Ed25519 keypair generation and secure local storage
- Node ID derived from public key (base58 encoding)
- Identity document served at `/.well-known/mesh/identity` and `/mesh/v1/identity`
- Self-signed identity document to prove key possession

**Record Signing & Verification**
- Sign lessons and checkpoints with Ed25519 using RFC 8785 canonical JSON
- Verify signatures on all incoming records before storing
- All fields present in the record (including graph metadata) included in canonical JSON
- Unknown fields preserved when re-announcing

**Visibility Controls**
- `private` / `unlisted` / `public` per lesson and checkpoint
- Default: `private` (no behavior change for existing users)
- `visibility` column added to existing lessons and checkpoints tables (migration)

**Publication & Revocation**
- Publish a record: sign it, send to connected peer
- Revoke a published record: sign revocation, send to peer, peer deletes cached copy
- Only original publisher can revoke

**MESH HTTP Endpoints (added to existing nellie-rs axum server)**
- `GET  /mesh/v1/identity` — node identity document
- `POST /mesh/v1/announce` — receive publication or revocation from peer
- `POST /mesh/v1/search` — receive and respond to search queries from peer
- `GET  /mesh/v1/peers` — list connected peers

**MeshEnvelope Wire Protocol**
- All messages wrapped: `{ version, type, timestamp, sender, payload, signature? }`

**Peer Management**
- Manual peer addition (exchange node ID + tunnel URL out-of-band)
- Store peer state: nodeId, endpoint URL, trust level, last seen, connected since
- Peer health check (periodic ping to verify liveness)

**Direct Trust (N=2, no transitive trust needed)**
- Trust is binary for MVP: you explicitly trust your peer or you don't
- Trust stored in local SQLite
- Untrusted records rejected

**Peer Search**
- Query your peer's public records directly (single hop, no TTL/gossip)
- Results include signed records + relevance score
- Merge peer results with local results, rank by relevance

**New MCP Tools (how agents actually use federation)**
- `mesh_publish` — publish a lesson or checkpoint (set visibility, sign, announce to peer)
- `mesh_search` — search local + peer records, return merged results
- `mesh_peers` — list, add, remove peers
- `mesh_trust` — add/remove trust for a peer
- `mesh_revoke` — revoke a previously published record
- `mesh_status` — federation status (identity, peer count, published record count)

**Schema Migration**
- Add `visibility` column to existing `lessons` table (default: `private`)
- Add `visibility` column to existing `checkpoints` table (default: `private`)
- New table: `mesh_identity` (keypair storage)
- New table: `mesh_peers` (peer connections and trust)
- New table: `mesh_remote_records` (cached records received from peers)
- New table: `mesh_revocations` (revocation records, retained to reject re-announcements)

### Nice-to-Have Features (v2)

- Web of Trust with transitive trust and configurable depth (needed when N>2)
- Multi-hop federated search with TTL, requestId deduplication, gossip propagation
- Directory server implementation (register, search, list nodes, submit records)
- Directory submission on publish (announce to directories, not just peers)
- Bulk sync protocol with cursor-based resumption and topic/recordType filtering
- Cryptographic revocation (AES-256-GCM encrypted records, revoke key = content dies)
- Key escrow with Shamir secret sharing
- End-to-end encryption for sensitive sync (X25519 + XChaCha20-Poly1305)
- Consumer and reader node tiers (read-only access without full node identity)
- LoRa/Meshtastic gateway integration for off-grid sensor networks
- TLS 1.3 enforcement at application layer (for deployments without tunnel)

## Technical Constraints

### Must Use

- Rust
- axum (HTTP server — extend nellie-rs's existing server)
- ed25519-dalek (Ed25519 signing/verification)
- tokio (async runtime — already in nellie-rs)
- serde + serde_json (serialization — already in nellie-rs)
- rusqlite (storage — extend nellie-rs's existing database)
- reqwest (HTTP client for outbound calls to peer)

### Cannot Use

- Any cloud-dependent services (Cloudflare Tunnel is infrastructure, not a dependency)
- Any centralized auth providers

## Other Constraints

- Must integrate into nellie-rs without breaking existing MCP tool interfaces (`add_lesson`, `search_lessons`, `add_checkpoint`, etc. continue to work unchanged)
- Must follow the MESH protocol spec in github.com/mmorris35/mesh-protocol
- Nodes are behind NAT on separate Tailscale networks — all connections route through Cloudflare Tunnel over the public internet
- Single binary deployment — mesh-node compiles into the nellie-rs binary, no sidecar
- Local-first — nellie-rs must function fully without any peers configured (graceful degradation, existing behavior preserved)
- Private keys must never be logged, transmitted, or leave the node
- Signature verification required before storing ANY remote record
- Schema migrations must be non-destructive (existing data preserved, new columns have safe defaults)

## Bootstrap Flow (How Two Nodes Connect)

1. Both operators run nellie-rs with mesh-node enabled
2. Both expose their node via Cloudflare Tunnel (get a public HTTPS URL)
3. Exchange node IDs and tunnel URLs out-of-band (Signal, email, in person)
4. Each operator adds the other as a peer: `mesh_peers add <nodeId> <tunnelUrl>`
5. Each operator trusts the other: `mesh_trust add <nodeId>`
6. Nodes verify identity by fetching `/.well-known/mesh/identity` and confirming the self-signed document
7. Federation is live — `mesh_publish`, `mesh_search`, `mesh_revoke` now work across both nodes

## Success Criteria

- Two nellie-rs instances on separate Tailscale networks can connect via Cloudflare Tunnel
- Node A can publish a lesson, Node B receives and stores it
- Node A can publish a checkpoint, Node B receives and stores it
- Node B can search Node A's public records via `mesh_search`
- Node A can revoke a published record, Node B deletes its cached copy
- Existing nellie-rs MCP tools (`add_lesson`, `search_lessons`, etc.) work unchanged
- Schema migration preserves all existing data
- All remote records have verified Ed25519 signatures before storage
- Test coverage >= 80%

---

*Generated by DevPlan MCP Server*
