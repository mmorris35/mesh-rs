pub mod envelope;
pub mod error;
pub mod http;
pub mod identity;
pub mod peer;
pub mod publish;
pub mod revoke;
pub mod search;
pub mod signing;
pub mod storage;
pub mod tools;
pub mod trust;
pub mod types;

use std::sync::{Arc, Mutex};

pub use envelope::MeshEnvelope;
pub use error::{ErrorResponse, MeshError, MeshResult};
pub use http::{mesh_router, MeshState};
pub use identity::NodeIdentity;
pub use peer::PeerManager;
pub use publish::Publisher;
pub use revoke::Revoker;
pub use search::FederatedSearch;
pub use tools::{MeshTools, RecordFetcher};
pub use trust::TrustManager;
pub use types::{
    PeerConnection, Publication, PublicationAnnouncement, RecordType, Revocation,
    RevocationAnnouncement, SearchFilters, SearchRequest, SearchResponse, SearchResult,
    SignatureBlock, SignedCheckpoint, SignedLesson, TrustLevel, Visibility,
};

/// Configuration for a MESH node.
#[derive(Debug, Clone)]
pub struct MeshConfig {
    pub mesh_endpoint: String,
    pub health_check_interval: std::time::Duration,
}

impl Default for MeshConfig {
    fn default() -> Self {
        Self {
            mesh_endpoint: "http://localhost:3000/mesh".to_string(),
            health_check_interval: std::time::Duration::from_secs(300), // 5 min
        }
    }
}

/// The main public API for a MESH federation node.
///
/// Orchestrates identity, peer management, trust, publishing, revocation,
/// federated search, and HTTP serving.
pub struct MeshNode {
    identity: Arc<NodeIdentity>,
    db: Arc<Mutex<rusqlite::Connection>>,
    peer_manager: Arc<PeerManager>,
    trust_manager: Arc<TrustManager>,
    #[allow(dead_code)]
    publisher: Arc<Publisher>,
    #[allow(dead_code)]
    revoker: Arc<Revoker>,
    #[allow(dead_code)]
    search: Arc<FederatedSearch>,
    tools: Arc<MeshTools>,
    config: MeshConfig,
}

impl MeshNode {
    /// Create a new `MeshNode` from a database connection and configuration.
    ///
    /// Runs schema migrations, loads or generates a node identity, and
    /// initializes all subsystems (peers, trust, publish, revoke, search, tools).
    pub fn new(conn: Arc<Mutex<rusqlite::Connection>>, config: MeshConfig) -> MeshResult<Self> {
        // Run migrations
        {
            let db = conn.lock().map_err(|e| {
                MeshError::StorageError(format!("failed to acquire database lock: {e}"))
            })?;
            storage::run_migrations(&db)?;
        }

        // Load or generate identity
        let identity = {
            let db = conn.lock().map_err(|e| {
                MeshError::StorageError(format!("failed to acquire database lock: {e}"))
            })?;
            if storage::MeshStorage::has_identity(&db)? {
                storage::MeshStorage::load_identity(&db)?
                    .ok_or_else(|| MeshError::StorageError("identity row missing".to_string()))?
            } else {
                let id = NodeIdentity::generate();
                storage::MeshStorage::save_identity(&db, &id)?;
                id
            }
        };
        let identity = Arc::new(identity);

        // Build HTTP client
        let http_client = reqwest::Client::new();

        // Initialize components
        let peer_manager = Arc::new(PeerManager::new(conn.clone(), http_client.clone()));
        let trust_manager = Arc::new(TrustManager::new(conn.clone()));
        let publisher = Arc::new(Publisher::new(
            identity.clone(),
            conn.clone(),
            http_client.clone(),
        ));
        let revoker = Arc::new(Revoker::new(
            identity.clone(),
            conn.clone(),
            http_client.clone(),
        ));
        let search = Arc::new(FederatedSearch::new(
            identity.clone(),
            conn.clone(),
            http_client,
        ));
        let tools = Arc::new(MeshTools::new(
            identity.clone(),
            conn.clone(),
            peer_manager.clone(),
            trust_manager.clone(),
            publisher.clone(),
            revoker.clone(),
            search.clone(),
        ));

        Ok(Self {
            identity,
            db: conn,
            peer_manager,
            trust_manager,
            publisher,
            revoker,
            search,
            tools,
            config,
        })
    }

    /// Build and return the Axum router for this node's HTTP API.
    pub fn router(&self) -> axum::Router {
        let state = Arc::new(MeshState {
            identity: self.identity.clone(),
            db: self.db.clone(),
            mesh_endpoint: self.config.mesh_endpoint.clone(),
        });
        mesh_router(state)
    }

    /// Reference to this node's cryptographic identity.
    pub fn identity(&self) -> &NodeIdentity {
        &self.identity
    }

    /// The base58-encoded node identifier.
    pub fn node_id(&self) -> &str {
        self.identity.node_id()
    }

    /// Reference to the high-level MCP tool handlers.
    pub fn tools(&self) -> &MeshTools {
        &self.tools
    }

    /// Reference to the peer manager.
    pub fn peer_manager(&self) -> &PeerManager {
        &self.peer_manager
    }

    /// Reference to the trust manager.
    pub fn trust_manager(&self) -> &TrustManager {
        &self.trust_manager
    }

    /// Spawn background tasks (health check loop) and return their join handles.
    pub fn spawn_background_tasks(&self) -> Vec<tokio::task::JoinHandle<()>> {
        let handle = self
            .peer_manager
            .clone()
            .spawn_health_check_loop(self.config.health_check_interval);
        vec![handle]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn in_memory_db() -> Arc<Mutex<Connection>> {
        Arc::new(Mutex::new(Connection::open_in_memory().unwrap()))
    }

    #[test]
    fn mesh_node_generates_identity_on_first_create() {
        let db = in_memory_db();
        let node = MeshNode::new(db, MeshConfig::default()).unwrap();
        let node_id = node.node_id();
        assert!(!node_id.is_empty());
        // Verify it decodes as valid base58
        let decoded = bs58::decode(node_id).into_vec().unwrap();
        assert_eq!(decoded.len(), 32);
    }

    #[test]
    fn mesh_node_loads_same_identity_from_same_db() {
        let db = in_memory_db();
        let node1 = MeshNode::new(db.clone(), MeshConfig::default()).unwrap();
        let id1 = node1.node_id().to_string();

        let node2 = MeshNode::new(db, MeshConfig::default()).unwrap();
        let id2 = node2.node_id().to_string();

        assert_eq!(id1, id2, "same DB should yield the same node identity");
    }

    #[test]
    fn mesh_node_router_returns_valid_router() {
        let db = in_memory_db();
        let node = MeshNode::new(db, MeshConfig::default()).unwrap();
        // Building the router should not panic
        let _router: axum::Router = node.router();
    }

    #[test]
    fn mesh_node_node_id_is_valid_base58() {
        let db = in_memory_db();
        let node = MeshNode::new(db, MeshConfig::default()).unwrap();
        let node_id = node.node_id();
        // Must be non-empty
        assert!(!node_id.is_empty());
        // Must decode as valid base58 to 32 bytes (Ed25519 public key)
        let decoded = bs58::decode(node_id).into_vec().unwrap();
        assert_eq!(decoded.len(), 32, "node_id should decode to 32 bytes");
        // All characters should be valid base58 (no 0, O, I, l)
        assert!(
            node_id.chars().all(|c| c.is_ascii_alphanumeric()
                && c != '0'
                && c != 'O'
                && c != 'I'
                && c != 'l'),
            "node_id should only contain valid base58 characters"
        );
    }

    #[test]
    fn mesh_node_accessor_methods_return_components() {
        let db = in_memory_db();
        let node = MeshNode::new(db, MeshConfig::default()).unwrap();
        // These should not panic
        let _identity = node.identity();
        let _tools = node.tools();
        let _pm = node.peer_manager();
        let _tm = node.trust_manager();
    }

    #[test]
    fn mesh_config_default_values() {
        let config = MeshConfig::default();
        assert_eq!(config.mesh_endpoint, "http://localhost:3000/mesh");
        assert_eq!(
            config.health_check_interval,
            std::time::Duration::from_secs(300)
        );
    }
}
