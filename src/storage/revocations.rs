use chrono::Utc;
use rusqlite::{params, Connection};

use crate::error::MeshResult;
use crate::storage::MeshStorage;

/// A revocation record stored locally after receiving it from the network.
#[derive(Debug, Clone)]
pub struct StoredRevocation {
    pub record_id: String,
    pub record_type: String,
    pub publisher_node_id: String,
    pub revocation_json: String,
    pub revoked_at: i64,
    pub received_at: i64,
}

impl MeshStorage {
    /// Store a revocation and delete the corresponding remote record (if any).
    pub fn store_revocation(
        conn: &Connection,
        record_id: &str,
        record_type: &str,
        publisher_node_id: &str,
        revocation_json: &str,
    ) -> MeshResult<()> {
        let now = Utc::now().timestamp();
        conn.execute(
            "INSERT OR REPLACE INTO mesh_revocations
                (record_id, record_type, publisher_node_id, revocation_json, revoked_at, received_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![record_id, record_type, publisher_node_id, revocation_json, now, now],
        )?;
        // Clean up any cached remote record.
        conn.execute(
            "DELETE FROM mesh_remote_records WHERE id = ?1",
            params![record_id],
        )?;
        Ok(())
    }

    /// Check whether a record has been revoked.
    pub fn is_revoked(conn: &Connection, record_id: &str) -> MeshResult<bool> {
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM mesh_revocations WHERE record_id = ?1",
            params![record_id],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    /// Get a stored revocation by record ID.
    pub fn get_revocation(
        conn: &Connection,
        record_id: &str,
    ) -> MeshResult<Option<StoredRevocation>> {
        let mut stmt = conn.prepare(
            "SELECT record_id, record_type, publisher_node_id, revocation_json, revoked_at, received_at
             FROM mesh_revocations WHERE record_id = ?1",
        )?;
        let mut rows = stmt.query_map(params![record_id], |row| {
            Ok(StoredRevocation {
                record_id: row.get(0)?,
                record_type: row.get(1)?,
                publisher_node_id: row.get(2)?,
                revocation_json: row.get(3)?,
                revoked_at: row.get(4)?,
                received_at: row.get(5)?,
            })
        })?;
        match rows.next() {
            Some(row) => Ok(Some(row?)),
            None => Ok(None),
        }
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
    fn is_revoked_returns_false_then_true_after_revocation() {
        let conn = setup();
        assert!(!MeshStorage::is_revoked(&conn, "rec-1").unwrap());

        MeshStorage::store_revocation(&conn, "rec-1", "lesson", "node-1", r#"{"reason":"old"}"#)
            .unwrap();

        assert!(MeshStorage::is_revoked(&conn, "rec-1").unwrap());
    }

    #[test]
    fn get_revocation_returns_none_then_some_after_store() {
        let conn = setup();
        assert!(MeshStorage::get_revocation(&conn, "rec-1")
            .unwrap()
            .is_none());

        MeshStorage::store_revocation(&conn, "rec-1", "lesson", "node-1", r#"{"reason":"old"}"#)
            .unwrap();

        let rev = MeshStorage::get_revocation(&conn, "rec-1")
            .unwrap()
            .unwrap();
        assert_eq!(rev.record_id, "rec-1");
        assert_eq!(rev.record_type, "lesson");
        assert_eq!(rev.publisher_node_id, "node-1");
        assert_eq!(rev.revocation_json, r#"{"reason":"old"}"#);
        assert!(rev.revoked_at > 0);
        assert!(rev.received_at > 0);
    }

    #[test]
    fn store_revocation_deletes_remote_record() {
        let conn = setup();
        // Insert a remote record first.
        conn.execute(
            "INSERT INTO mesh_remote_records (id, record_type, publisher_node_id, signed_record, visibility, received_at)
             VALUES ('rec-1', 'lesson', 'node-1', '{}', 'public', 1000)",
            [],
        )
        .unwrap();

        // Verify it exists.
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM mesh_remote_records WHERE id = 'rec-1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);

        // Store revocation.
        MeshStorage::store_revocation(&conn, "rec-1", "lesson", "node-1", r#"{"reason":"old"}"#)
            .unwrap();

        // Remote record should be deleted.
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM mesh_remote_records WHERE id = 'rec-1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);

        // Revocation should exist.
        assert!(MeshStorage::is_revoked(&conn, "rec-1").unwrap());
    }
}
