use chrono::Utc;
use rusqlite::{params, Connection};

use crate::error::{MeshError, MeshResult};
use crate::storage::MeshStorage;
use crate::types::{PeerConnection, TrustLevel};

impl MeshStorage {
    /// Insert a new peer with trust_level='none'. Does nothing if the peer already exists.
    pub fn add_peer(conn: &Connection, node_id: &str, endpoint: &str) -> MeshResult<()> {
        conn.execute(
            "INSERT OR IGNORE INTO mesh_peers (node_id, endpoint, trust_level, created_at)
             VALUES (?1, ?2, 'none', ?3)",
            params![node_id, endpoint, Utc::now().timestamp()],
        )?;
        Ok(())
    }

    /// Remove a peer and all its remote records.
    pub fn remove_peer(conn: &Connection, node_id: &str) -> MeshResult<()> {
        conn.execute(
            "DELETE FROM mesh_remote_records WHERE publisher_node_id = ?1",
            params![node_id],
        )?;
        conn.execute(
            "DELETE FROM mesh_peers WHERE node_id = ?1",
            params![node_id],
        )?;
        Ok(())
    }

    /// List all known peers.
    pub fn list_peers(conn: &Connection) -> MeshResult<Vec<PeerConnection>> {
        let mut stmt = conn.prepare(
            "SELECT node_id, endpoint, trust_level, last_seen, connected_since FROM mesh_peers",
        )?;
        let rows = stmt.query_map([], |row| {
            let trust_str: String = row.get(2)?;
            Ok(PeerConnection {
                node_id: row.get(0)?,
                endpoint: row.get(1)?,
                trust_level: if trust_str == "full" {
                    TrustLevel::Full
                } else {
                    TrustLevel::None
                },
                last_seen: row.get(3)?,
                connected_since: row.get(4)?,
            })
        })?;
        let mut peers = Vec::new();
        for row in rows {
            peers.push(row?);
        }
        Ok(peers)
    }

    /// Get a single peer by node_id.
    pub fn get_peer(conn: &Connection, node_id: &str) -> MeshResult<Option<PeerConnection>> {
        let mut stmt = conn.prepare(
            "SELECT node_id, endpoint, trust_level, last_seen, connected_since
             FROM mesh_peers WHERE node_id = ?1",
        )?;
        let mut rows = stmt.query_map(params![node_id], |row| {
            let trust_str: String = row.get(2)?;
            Ok(PeerConnection {
                node_id: row.get(0)?,
                endpoint: row.get(1)?,
                trust_level: if trust_str == "full" {
                    TrustLevel::Full
                } else {
                    TrustLevel::None
                },
                last_seen: row.get(3)?,
                connected_since: row.get(4)?,
            })
        })?;
        match rows.next() {
            Some(row) => Ok(Some(row?)),
            None => Ok(None),
        }
    }

    /// Set the trust level for an existing peer. Returns error if peer not found.
    pub fn set_trust(conn: &Connection, node_id: &str, level: TrustLevel) -> MeshResult<()> {
        let level_str = match level {
            TrustLevel::Full => "full",
            TrustLevel::None => "none",
        };
        let updated = conn.execute(
            "UPDATE mesh_peers SET trust_level = ?1 WHERE node_id = ?2",
            params![level_str, node_id],
        )?;
        if updated == 0 {
            return Err(MeshError::InvalidRequest("peer not found".to_string()));
        }
        Ok(())
    }

    /// Update the last_seen timestamp for a peer. On first connection, also sets connected_since.
    pub fn update_last_seen(conn: &Connection, node_id: &str, timestamp: i64) -> MeshResult<()> {
        conn.execute(
            "UPDATE mesh_peers SET last_seen = ?1,
                connected_since = CASE WHEN connected_since IS NULL THEN ?1 ELSE connected_since END
             WHERE node_id = ?2",
            params![timestamp, node_id],
        )?;
        Ok(())
    }

    /// Get only peers with trust_level = 'full'.
    pub fn get_trusted_peers(conn: &Connection) -> MeshResult<Vec<PeerConnection>> {
        let mut stmt = conn.prepare(
            "SELECT node_id, endpoint, trust_level, last_seen, connected_since
             FROM mesh_peers WHERE trust_level = 'full'",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(PeerConnection {
                node_id: row.get(0)?,
                endpoint: row.get(1)?,
                trust_level: TrustLevel::Full,
                last_seen: row.get(3)?,
                connected_since: row.get(4)?,
            })
        })?;
        let mut peers = Vec::new();
        for row in rows {
            peers.push(row?);
        }
        Ok(peers)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::run_migrations;
    use rusqlite::Connection;

    fn setup() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();
        conn
    }

    #[test]
    fn add_peer_and_list() {
        let conn = setup();
        MeshStorage::add_peer(&conn, "node-1", "https://node1.example.com").unwrap();
        let peers = MeshStorage::list_peers(&conn).unwrap();
        assert_eq!(peers.len(), 1);
        assert_eq!(peers[0].node_id, "node-1");
        assert_eq!(peers[0].endpoint, "https://node1.example.com");
        assert_eq!(peers[0].trust_level, TrustLevel::None);
    }

    #[test]
    fn remove_peer_excludes_from_list() {
        let conn = setup();
        MeshStorage::add_peer(&conn, "node-1", "https://node1.example.com").unwrap();
        MeshStorage::remove_peer(&conn, "node-1").unwrap();
        let peers = MeshStorage::list_peers(&conn).unwrap();
        assert!(peers.is_empty());
    }

    #[test]
    fn remove_peer_cascades_to_remote_records() {
        let conn = setup();
        MeshStorage::add_peer(&conn, "node-1", "https://node1.example.com").unwrap();
        // Insert a remote record for this peer.
        conn.execute(
            "INSERT INTO mesh_remote_records (id, record_type, publisher_node_id, signed_record, visibility, received_at)
             VALUES ('rec-1', 'lesson', 'node-1', '{}', 'public', 1000)",
            [],
        )
        .unwrap();
        // Verify it exists.
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM mesh_remote_records WHERE publisher_node_id = 'node-1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);

        MeshStorage::remove_peer(&conn, "node-1").unwrap();

        // Verify remote records are deleted.
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM mesh_remote_records WHERE publisher_node_id = 'node-1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn set_trust_persists() {
        let conn = setup();
        MeshStorage::add_peer(&conn, "node-1", "https://node1.example.com").unwrap();
        MeshStorage::set_trust(&conn, "node-1", TrustLevel::Full).unwrap();
        let peer = MeshStorage::get_peer(&conn, "node-1").unwrap().unwrap();
        assert_eq!(peer.trust_level, TrustLevel::Full);
    }

    #[test]
    fn set_trust_nonexistent_peer_returns_error() {
        let conn = setup();
        let result = MeshStorage::set_trust(&conn, "no-such-node", TrustLevel::Full);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, MeshError::InvalidRequest(_)));
        assert!(err.to_string().contains("peer not found"));
    }

    #[test]
    fn update_last_seen_sets_timestamp_and_connected_since() {
        let conn = setup();
        MeshStorage::add_peer(&conn, "node-1", "https://node1.example.com").unwrap();

        // First call should set both last_seen and connected_since.
        MeshStorage::update_last_seen(&conn, "node-1", 1000).unwrap();
        let peer = MeshStorage::get_peer(&conn, "node-1").unwrap().unwrap();
        assert_eq!(peer.last_seen, Some(1000));
        assert_eq!(peer.connected_since, Some(1000));

        // Second call should update last_seen but NOT change connected_since.
        MeshStorage::update_last_seen(&conn, "node-1", 2000).unwrap();
        let peer = MeshStorage::get_peer(&conn, "node-1").unwrap().unwrap();
        assert_eq!(peer.last_seen, Some(2000));
        assert_eq!(peer.connected_since, Some(1000));
    }

    #[test]
    fn get_trusted_peers_filters_correctly() {
        let conn = setup();
        MeshStorage::add_peer(&conn, "node-1", "https://node1.example.com").unwrap();
        MeshStorage::add_peer(&conn, "node-2", "https://node2.example.com").unwrap();
        MeshStorage::set_trust(&conn, "node-1", TrustLevel::Full).unwrap();

        let trusted = MeshStorage::get_trusted_peers(&conn).unwrap();
        assert_eq!(trusted.len(), 1);
        assert_eq!(trusted[0].node_id, "node-1");
        assert_eq!(trusted[0].trust_level, TrustLevel::Full);
    }
}
