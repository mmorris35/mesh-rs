use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::Utc;
use rusqlite::Connection;
use tracing::{info, warn};

use crate::error::{MeshError, MeshResult};
use crate::identity::IdentityDocument;
use crate::storage::MeshStorage;
use crate::types::PeerConnection;

/// Manages peer connections for this MESH node.
pub struct PeerManager {
    db: Arc<Mutex<Connection>>,
    #[allow(dead_code)]
    http_client: reqwest::Client,
}

impl PeerManager {
    /// Create a new `PeerManager` backed by the given database connection and HTTP client.
    pub fn new(db: Arc<Mutex<Connection>>, http_client: reqwest::Client) -> Self {
        Self { db, http_client }
    }

    /// Add a peer with the given node ID and endpoint URL.
    ///
    /// The endpoint must be an HTTP or HTTPS URL; otherwise an error is returned.
    pub fn add_peer(&self, node_id: &str, endpoint: &str) -> MeshResult<()> {
        if !endpoint.starts_with("http://") && !endpoint.starts_with("https://") {
            return Err(MeshError::InvalidRequest(
                "endpoint must be an HTTP(S) URL".into(),
            ));
        }
        let conn = self.db.lock().map_err(|e| {
            MeshError::StorageError(format!("failed to acquire database lock: {e}"))
        })?;
        MeshStorage::add_peer(&conn, node_id, endpoint)
    }

    /// Remove a peer by node ID.
    pub fn remove_peer(&self, node_id: &str) -> MeshResult<()> {
        let conn = self.db.lock().map_err(|e| {
            MeshError::StorageError(format!("failed to acquire database lock: {e}"))
        })?;
        MeshStorage::remove_peer(&conn, node_id)
    }

    /// List all known peers.
    pub fn list_peers(&self) -> MeshResult<Vec<PeerConnection>> {
        let conn = self.db.lock().map_err(|e| {
            MeshError::StorageError(format!("failed to acquire database lock: {e}"))
        })?;
        MeshStorage::list_peers(&conn)
    }

    /// Get a single peer by node ID, or `None` if not found.
    pub fn get_peer(&self, node_id: &str) -> MeshResult<Option<PeerConnection>> {
        let conn = self.db.lock().map_err(|e| {
            MeshError::StorageError(format!("failed to acquire database lock: {e}"))
        })?;
        MeshStorage::get_peer(&conn, node_id)
    }

    /// Returns `true` if the given node ID corresponds to a known peer.
    pub fn is_known_peer(&self, node_id: &str) -> MeshResult<bool> {
        Ok(self.get_peer(node_id)?.is_some())
    }

    /// Fetch and verify a peer's identity document from its well-known endpoint.
    ///
    /// Retrieves `{peer.endpoint}/.well-known/mesh/identity`, verifies the self-signature,
    /// and confirms that the `node_id` in the document matches the stored peer ID.
    /// On success, updates the peer's `last_seen` timestamp and returns the document.
    pub async fn verify_peer_identity(&self, node_id: &str) -> MeshResult<IdentityDocument> {
        // Look up peer in storage
        let peer = self
            .get_peer(node_id)?
            .ok_or_else(|| MeshError::InvalidRequest(format!("peer not found: {node_id}")))?;

        // Build the identity URL
        let url = format!(
            "{}/.well-known/mesh/identity",
            peer.endpoint.trim_end_matches('/')
        );

        // Fetch with timeouts: 10s connect, 30s total
        let response = self
            .http_client
            .get(&url)
            .timeout(Duration::from_secs(30))
            .send()
            .await
            .map_err(|e| MeshError::NetworkError(format!("failed to reach peer {node_id}: {e}")))?;

        let identity_doc: IdentityDocument = response.json().await.map_err(|e| {
            MeshError::NetworkError(format!(
                "invalid identity response from peer {node_id}: {e}"
            ))
        })?;

        // Verify the self-signature
        identity_doc.verify()?;

        // Verify node_id matches what we have stored
        if identity_doc.node_id != node_id {
            return Err(MeshError::IdentityError(format!(
                "node_id mismatch: expected {node_id}, got {}",
                identity_doc.node_id
            )));
        }

        // Update last_seen timestamp
        let now = Utc::now().timestamp();
        let conn = self.db.lock().map_err(|e| {
            MeshError::StorageError(format!("failed to acquire database lock: {e}"))
        })?;
        MeshStorage::update_last_seen(&conn, node_id, now)?;

        Ok(identity_doc)
    }

    /// Perform a health check on a peer by verifying its identity.
    ///
    /// Returns `Ok(true)` if the peer is reachable and its identity is valid,
    /// `Ok(false)` if the peer is unreachable (network error), or propagates
    /// other errors (e.g., identity mismatch, storage failure).
    pub async fn health_check(&self, node_id: &str) -> MeshResult<bool> {
        match self.verify_peer_identity(node_id).await {
            Ok(_) => Ok(true),
            Err(MeshError::NetworkError(_)) => Ok(false),
            Err(e) => Err(e),
        }
    }

    /// Spawn a background task that periodically health-checks all known peers.
    ///
    /// The task runs on the tokio runtime until the returned `JoinHandle` is dropped
    /// or aborted. On each tick it lists all peers and performs a health check on each.
    pub fn spawn_health_check_loop(
        self: Arc<Self>,
        interval: Duration,
    ) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            loop {
                ticker.tick().await;
                let peers = match self.list_peers() {
                    Ok(p) => p,
                    Err(e) => {
                        warn!("health check: failed to list peers: {e}");
                        continue;
                    }
                };
                for peer in &peers {
                    match self.health_check(&peer.node_id).await {
                        Ok(true) => {
                            info!(node_id = %peer.node_id, "health check: peer is healthy");
                        }
                        Ok(false) => {
                            warn!(node_id = %peer.node_id, "health check: peer is unreachable");
                        }
                        Err(e) => {
                            warn!(node_id = %peer.node_id, error = %e, "health check: peer error");
                        }
                    }
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::run_migrations;

    fn setup() -> PeerManager {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();
        PeerManager::new(Arc::new(Mutex::new(conn)), reqwest::Client::new())
    }

    #[test]
    fn add_peer_and_list() {
        let pm = setup();
        pm.add_peer("node-1", "https://node1.example.com").unwrap();
        let peers = pm.list_peers().unwrap();
        assert_eq!(peers.len(), 1);
        assert_eq!(peers[0].node_id, "node-1");
        assert_eq!(peers[0].endpoint, "https://node1.example.com");
    }

    #[test]
    fn remove_peer_removes_from_list() {
        let pm = setup();
        pm.add_peer("node-1", "https://node1.example.com").unwrap();
        pm.remove_peer("node-1").unwrap();
        let peers = pm.list_peers().unwrap();
        assert!(peers.is_empty());
    }

    #[test]
    fn add_peer_invalid_url_errors() {
        let pm = setup();
        let result = pm.add_peer("node-1", "not a url");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, MeshError::InvalidRequest(_)));
        assert!(err.to_string().contains("HTTP(S) URL"));
    }

    #[test]
    fn get_nonexistent_peer_returns_none() {
        let pm = setup();
        let peer = pm.get_peer("no-such-node").unwrap();
        assert!(peer.is_none());
    }

    #[test]
    fn is_known_peer_false_then_true() {
        let pm = setup();
        assert!(!pm.is_known_peer("node-1").unwrap());
        pm.add_peer("node-1", "https://node1.example.com").unwrap();
        assert!(pm.is_known_peer("node-1").unwrap());
    }

    #[tokio::test]
    async fn verify_peer_identity_nonexistent_peer_errors() {
        let pm = setup();
        let result = pm.verify_peer_identity("no-such-node").await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, MeshError::InvalidRequest(_)));
        assert!(err.to_string().contains("peer not found"));
    }

    #[tokio::test]
    async fn health_check_nonexistent_peer_errors() {
        let pm = setup();
        let result = pm.health_check("no-such-node").await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        // Non-existent peer is an InvalidRequest, not a NetworkError, so it propagates
        assert!(matches!(err, MeshError::InvalidRequest(_)));
    }

    #[tokio::test]
    async fn health_check_unreachable_peer_returns_false() {
        let pm = setup();
        // Add a peer with an endpoint that will fail to connect
        pm.add_peer("node-1", "http://127.0.0.1:1").unwrap();
        let result = pm.health_check("node-1").await;
        assert!(result.is_ok());
        assert!(!result.unwrap());
    }

    #[tokio::test]
    async fn spawn_health_check_loop_can_be_aborted() {
        let pm = setup();
        let pm = Arc::new(pm);
        let handle = pm
            .clone()
            .spawn_health_check_loop(std::time::Duration::from_secs(3600));
        // The handle should be abortable
        handle.abort();
        let result = handle.await;
        assert!(result.is_err()); // JoinError from abort
    }
}
