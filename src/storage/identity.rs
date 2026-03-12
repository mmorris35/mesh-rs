use chrono::Utc;
use rusqlite::Connection;

use crate::error::{MeshError, MeshResult};
use crate::identity::NodeIdentity;
use crate::storage::MeshStorage;

impl MeshStorage {
    /// Persist a node identity as the singleton row (id = 1) in `mesh_identity`.
    pub fn save_identity(conn: &Connection, identity: &NodeIdentity) -> MeshResult<()> {
        let private_key = identity.private_key_bytes().as_slice();
        let public_key = identity.verifying_key().to_bytes();
        let node_id = identity.node_id();
        let fingerprint = identity.fingerprint();
        let created_at = Utc::now().timestamp_millis();

        conn.execute(
            "INSERT OR REPLACE INTO mesh_identity (id, private_key, public_key, node_id, fingerprint, created_at)
             VALUES (1, ?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![private_key, public_key.as_slice(), node_id, fingerprint, created_at],
        )?;

        Ok(())
    }

    /// Load the singleton node identity, if one has been saved.
    pub fn load_identity(conn: &Connection) -> MeshResult<Option<NodeIdentity>> {
        let mut stmt = conn.prepare("SELECT private_key FROM mesh_identity WHERE id = 1")?;

        let mut rows = stmt.query([])?;
        let Some(row) = rows.next()? else {
            return Ok(None);
        };

        let private_key_blob: Vec<u8> = row.get(0)?;
        let private_key_bytes: [u8; 32] = private_key_blob.try_into().map_err(|_| {
            MeshError::StorageError("stored private key is not 32 bytes".to_string())
        })?;

        let identity = NodeIdentity::from_private_key_bytes(&private_key_bytes)?;
        Ok(Some(identity))
    }

    /// Check whether a node identity has been saved.
    pub fn has_identity(conn: &Connection) -> MeshResult<bool> {
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM mesh_identity WHERE id = 1",
            [],
            |row| row.get(0),
        )?;
        Ok(count > 0)
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
    fn save_and_load_identity_round_trip() {
        let conn = setup();
        let identity = NodeIdentity::generate();
        MeshStorage::save_identity(&conn, &identity).unwrap();

        let loaded = MeshStorage::load_identity(&conn)
            .unwrap()
            .expect("should have identity");
        assert_eq!(loaded.node_id(), identity.node_id());
        assert_eq!(loaded.fingerprint(), identity.fingerprint());
        assert_eq!(loaded.public_key_base64(), identity.public_key_base64());
    }

    #[test]
    fn load_from_empty_returns_none() {
        let conn = setup();
        let loaded = MeshStorage::load_identity(&conn).unwrap();
        assert!(loaded.is_none());
    }

    #[test]
    fn has_identity_false_then_true() {
        let conn = setup();
        assert!(!MeshStorage::has_identity(&conn).unwrap());

        let identity = NodeIdentity::generate();
        MeshStorage::save_identity(&conn, &identity).unwrap();
        assert!(MeshStorage::has_identity(&conn).unwrap());
    }

    #[test]
    fn save_twice_overwrites_singleton() {
        let conn = setup();
        let identity = NodeIdentity::generate();

        MeshStorage::save_identity(&conn, &identity).unwrap();
        MeshStorage::save_identity(&conn, &identity).unwrap();

        let loaded = MeshStorage::load_identity(&conn)
            .unwrap()
            .expect("should have identity");
        assert_eq!(loaded.node_id(), identity.node_id());

        // Verify only one row exists
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM mesh_identity", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }
}
