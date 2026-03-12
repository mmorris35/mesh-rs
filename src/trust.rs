use std::sync::{Arc, Mutex};

use rusqlite::Connection;

use crate::error::{MeshError, MeshResult};
use crate::storage::MeshStorage;
use crate::types::{PeerConnection, TrustLevel};

/// Manages trust relationships between mesh peers.
pub struct TrustManager {
    db: Arc<Mutex<Connection>>,
}

impl TrustManager {
    /// Create a new `TrustManager` backed by the given database connection.
    pub fn new(db: Arc<Mutex<Connection>>) -> Self {
        Self { db }
    }

    /// Grant full trust to a peer identified by `node_id`.
    ///
    /// Returns an error if the peer does not exist.
    pub fn add_trust(&self, node_id: &str) -> MeshResult<()> {
        let conn = self
            .db
            .lock()
            .map_err(|e| MeshError::StorageError(format!("failed to acquire db lock: {e}")))?;
        MeshStorage::set_trust(&conn, node_id, TrustLevel::Full)
    }

    /// Remove trust from a peer, setting its trust level to `None`.
    ///
    /// Returns an error if the peer does not exist.
    pub fn remove_trust(&self, node_id: &str) -> MeshResult<()> {
        let conn = self
            .db
            .lock()
            .map_err(|e| MeshError::StorageError(format!("failed to acquire db lock: {e}")))?;
        MeshStorage::set_trust(&conn, node_id, TrustLevel::None)
    }

    /// Check whether the given peer is fully trusted.
    ///
    /// Returns `false` (not an error) if the peer does not exist.
    pub fn is_trusted(&self, node_id: &str) -> MeshResult<bool> {
        let conn = self
            .db
            .lock()
            .map_err(|e| MeshError::StorageError(format!("failed to acquire db lock: {e}")))?;
        match MeshStorage::get_peer(&conn, node_id)? {
            Some(peer) => Ok(peer.trust_level == TrustLevel::Full),
            None => Ok(false),
        }
    }

    /// List all peers that have full trust.
    pub fn list_trusted(&self) -> MeshResult<Vec<PeerConnection>> {
        let conn = self
            .db
            .lock()
            .map_err(|e| MeshError::StorageError(format!("failed to acquire db lock: {e}")))?;
        MeshStorage::get_trusted_peers(&conn)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::run_migrations;

    fn setup() -> TrustManager {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();
        let db = Arc::new(Mutex::new(conn));
        TrustManager::new(db)
    }

    #[test]
    fn add_trust_then_is_trusted() {
        let tm = setup();
        let db = tm.db.lock().unwrap();
        MeshStorage::add_peer(&db, "node-1", "https://node1.example.com").unwrap();
        drop(db);

        tm.add_trust("node-1").unwrap();
        assert!(tm.is_trusted("node-1").unwrap());
    }

    #[test]
    fn remove_trust_then_not_trusted() {
        let tm = setup();
        let db = tm.db.lock().unwrap();
        MeshStorage::add_peer(&db, "node-1", "https://node1.example.com").unwrap();
        drop(db);

        tm.add_trust("node-1").unwrap();
        assert!(tm.is_trusted("node-1").unwrap());

        tm.remove_trust("node-1").unwrap();
        assert!(!tm.is_trusted("node-1").unwrap());
    }

    #[test]
    fn add_trust_nonexistent_peer_returns_error() {
        let tm = setup();
        let result = tm.add_trust("no-such-node");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, MeshError::InvalidRequest(_)));
        assert!(err.to_string().contains("peer not found"));
    }

    #[test]
    fn list_trusted_only_returns_trusted_peers() {
        let tm = setup();
        {
            let db = tm.db.lock().unwrap();
            MeshStorage::add_peer(&db, "node-1", "https://node1.example.com").unwrap();
            MeshStorage::add_peer(&db, "node-2", "https://node2.example.com").unwrap();
        }

        tm.add_trust("node-1").unwrap();

        let trusted = tm.list_trusted().unwrap();
        assert_eq!(trusted.len(), 1);
        assert_eq!(trusted[0].node_id, "node-1");
        assert_eq!(trusted[0].trust_level, TrustLevel::Full);
    }

    #[test]
    fn is_trusted_unknown_peer_returns_false() {
        let tm = setup();
        assert!(!tm.is_trusted("unknown-node").unwrap());
    }
}
