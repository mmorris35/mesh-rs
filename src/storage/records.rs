use chrono::Utc;
use rusqlite::{params, Connection};

use crate::error::{MeshError, MeshResult};
use crate::storage::MeshStorage;

/// A remote record cached locally after receiving it from the network.
#[derive(Debug, Clone)]
pub struct RemoteRecord {
    pub id: String,
    pub record_type: String,
    pub publisher_node_id: String,
    pub signed_record: String,
    pub visibility: String,
    pub received_at: i64,
}

impl MeshStorage {
    /// Store a remote record. Returns `AlreadyRevoked` if the record has been revoked.
    pub fn store_remote_record(
        conn: &Connection,
        id: &str,
        record_type: &str,
        publisher_node_id: &str,
        signed_record_json: &str,
        visibility: &str,
    ) -> MeshResult<()> {
        if MeshStorage::is_revoked(conn, id)? {
            return Err(MeshError::AlreadyRevoked(id.to_string()));
        }
        let now = Utc::now().timestamp();
        conn.execute(
            "INSERT OR REPLACE INTO mesh_remote_records
                (id, record_type, publisher_node_id, signed_record, visibility, received_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                id,
                record_type,
                publisher_node_id,
                signed_record_json,
                visibility,
                now
            ],
        )?;
        Ok(())
    }

    /// Get a single remote record by ID.
    pub fn get_remote_record(conn: &Connection, id: &str) -> MeshResult<Option<RemoteRecord>> {
        let mut stmt = conn.prepare(
            "SELECT id, record_type, publisher_node_id, signed_record, visibility, received_at
             FROM mesh_remote_records WHERE id = ?1",
        )?;
        let mut rows = stmt.query_map(params![id], |row| {
            Ok(RemoteRecord {
                id: row.get(0)?,
                record_type: row.get(1)?,
                publisher_node_id: row.get(2)?,
                signed_record: row.get(3)?,
                visibility: row.get(4)?,
                received_at: row.get(5)?,
            })
        })?;
        match rows.next() {
            Some(row) => Ok(Some(row?)),
            None => Ok(None),
        }
    }

    /// Delete a remote record by ID.
    pub fn delete_remote_record(conn: &Connection, id: &str) -> MeshResult<()> {
        conn.execute("DELETE FROM mesh_remote_records WHERE id = ?1", params![id])?;
        Ok(())
    }

    /// Search remote records by text query (LIKE on signed_record JSON),
    /// optionally filtering by record_type, with a result limit.
    pub fn search_remote_records(
        conn: &Connection,
        query: &str,
        record_type: Option<&str>,
        limit: i64,
    ) -> MeshResult<Vec<RemoteRecord>> {
        let like_pattern = format!("%{query}%");
        let (sql, params_vec): (String, Vec<Box<dyn rusqlite::types::ToSql>>) = match record_type {
            Some(rt) => (
                "SELECT id, record_type, publisher_node_id, signed_record, visibility, received_at
                 FROM mesh_remote_records
                 WHERE signed_record LIKE ?1 AND record_type = ?2
                 LIMIT ?3"
                    .to_string(),
                vec![
                    Box::new(like_pattern),
                    Box::new(rt.to_string()),
                    Box::new(limit),
                ],
            ),
            None => (
                "SELECT id, record_type, publisher_node_id, signed_record, visibility, received_at
                 FROM mesh_remote_records
                 WHERE signed_record LIKE ?1
                 LIMIT ?2"
                    .to_string(),
                vec![Box::new(like_pattern), Box::new(limit)],
            ),
        };
        let mut stmt = conn.prepare(&sql)?;
        let params_refs: Vec<&dyn rusqlite::types::ToSql> =
            params_vec.iter().map(|p| p.as_ref()).collect();
        let rows = stmt.query_map(params_refs.as_slice(), |row| {
            Ok(RemoteRecord {
                id: row.get(0)?,
                record_type: row.get(1)?,
                publisher_node_id: row.get(2)?,
                signed_record: row.get(3)?,
                visibility: row.get(4)?,
                received_at: row.get(5)?,
            })
        })?;
        let mut records = Vec::new();
        for row in rows {
            records.push(row?);
        }
        Ok(records)
    }

    /// List all remote records from a specific publisher.
    pub fn list_remote_records_by_publisher(
        conn: &Connection,
        node_id: &str,
    ) -> MeshResult<Vec<RemoteRecord>> {
        let mut stmt = conn.prepare(
            "SELECT id, record_type, publisher_node_id, signed_record, visibility, received_at
             FROM mesh_remote_records WHERE publisher_node_id = ?1",
        )?;
        let rows = stmt.query_map(params![node_id], |row| {
            Ok(RemoteRecord {
                id: row.get(0)?,
                record_type: row.get(1)?,
                publisher_node_id: row.get(2)?,
                signed_record: row.get(3)?,
                visibility: row.get(4)?,
                received_at: row.get(5)?,
            })
        })?;
        let mut records = Vec::new();
        for row in rows {
            records.push(row?);
        }
        Ok(records)
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
    fn store_and_retrieve_remote_record() {
        let conn = setup();
        MeshStorage::store_remote_record(
            &conn,
            "rec-1",
            "lesson",
            "node-1",
            r#"{"title":"Intro to Rust"}"#,
            "public",
        )
        .unwrap();

        let rec = MeshStorage::get_remote_record(&conn, "rec-1")
            .unwrap()
            .unwrap();
        assert_eq!(rec.id, "rec-1");
        assert_eq!(rec.record_type, "lesson");
        assert_eq!(rec.publisher_node_id, "node-1");
        assert_eq!(rec.signed_record, r#"{"title":"Intro to Rust"}"#);
        assert_eq!(rec.visibility, "public");
        assert!(rec.received_at > 0);
    }

    #[test]
    fn store_revoked_record_returns_already_revoked() {
        let conn = setup();
        // Revoke first.
        MeshStorage::store_revocation(&conn, "rec-1", "lesson", "node-1", r#"{"reason":"old"}"#)
            .unwrap();

        // Attempt to store should fail.
        let result = MeshStorage::store_remote_record(
            &conn,
            "rec-1",
            "lesson",
            "node-1",
            r#"{"title":"Intro"}"#,
            "public",
        );
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, MeshError::AlreadyRevoked(_)));
        assert!(err.to_string().contains("rec-1"));
    }

    #[test]
    fn store_then_revoke_deletes_record() {
        let conn = setup();
        MeshStorage::store_remote_record(
            &conn,
            "rec-1",
            "lesson",
            "node-1",
            r#"{"title":"Intro"}"#,
            "public",
        )
        .unwrap();

        // Record exists.
        assert!(MeshStorage::get_remote_record(&conn, "rec-1")
            .unwrap()
            .is_some());

        // Revoke it.
        MeshStorage::store_revocation(&conn, "rec-1", "lesson", "node-1", r#"{"reason":"old"}"#)
            .unwrap();

        // Record should be gone.
        assert!(MeshStorage::get_remote_record(&conn, "rec-1")
            .unwrap()
            .is_none());

        // Revocation should be stored.
        assert!(MeshStorage::is_revoked(&conn, "rec-1").unwrap());
    }

    #[test]
    fn search_records_by_text_query() {
        let conn = setup();
        MeshStorage::store_remote_record(
            &conn,
            "rec-1",
            "lesson",
            "node-1",
            r#"{"title":"Intro to Rust"}"#,
            "public",
        )
        .unwrap();
        MeshStorage::store_remote_record(
            &conn,
            "rec-2",
            "lesson",
            "node-1",
            r#"{"title":"Advanced Python"}"#,
            "public",
        )
        .unwrap();
        MeshStorage::store_remote_record(
            &conn,
            "rec-3",
            "checkpoint",
            "node-2",
            r#"{"title":"Rust Quiz"}"#,
            "public",
        )
        .unwrap();

        let results = MeshStorage::search_remote_records(&conn, "Rust", None, 10).unwrap();
        assert_eq!(results.len(), 2);

        let results = MeshStorage::search_remote_records(&conn, "Python", None, 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "rec-2");
    }

    #[test]
    fn search_records_by_type_filter() {
        let conn = setup();
        MeshStorage::store_remote_record(
            &conn,
            "rec-1",
            "lesson",
            "node-1",
            r#"{"title":"Intro to Rust"}"#,
            "public",
        )
        .unwrap();
        MeshStorage::store_remote_record(
            &conn,
            "rec-2",
            "checkpoint",
            "node-1",
            r#"{"title":"Rust Quiz"}"#,
            "public",
        )
        .unwrap();

        let results =
            MeshStorage::search_remote_records(&conn, "Rust", Some("lesson"), 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "rec-1");

        let results =
            MeshStorage::search_remote_records(&conn, "Rust", Some("checkpoint"), 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "rec-2");
    }

    #[test]
    fn delete_record_removes_from_db() {
        let conn = setup();
        MeshStorage::store_remote_record(
            &conn,
            "rec-1",
            "lesson",
            "node-1",
            r#"{"title":"Intro"}"#,
            "public",
        )
        .unwrap();

        MeshStorage::delete_remote_record(&conn, "rec-1").unwrap();

        assert!(MeshStorage::get_remote_record(&conn, "rec-1")
            .unwrap()
            .is_none());
    }

    #[test]
    fn list_records_by_publisher() {
        let conn = setup();
        MeshStorage::store_remote_record(
            &conn,
            "rec-1",
            "lesson",
            "node-1",
            r#"{"title":"A"}"#,
            "public",
        )
        .unwrap();
        MeshStorage::store_remote_record(
            &conn,
            "rec-2",
            "lesson",
            "node-1",
            r#"{"title":"B"}"#,
            "public",
        )
        .unwrap();
        MeshStorage::store_remote_record(
            &conn,
            "rec-3",
            "lesson",
            "node-2",
            r#"{"title":"C"}"#,
            "public",
        )
        .unwrap();

        let node1_records = MeshStorage::list_remote_records_by_publisher(&conn, "node-1").unwrap();
        assert_eq!(node1_records.len(), 2);

        let node2_records = MeshStorage::list_remote_records_by_publisher(&conn, "node-2").unwrap();
        assert_eq!(node2_records.len(), 1);
        assert_eq!(node2_records[0].id, "rec-3");

        let node3_records = MeshStorage::list_remote_records_by_publisher(&conn, "node-3").unwrap();
        assert!(node3_records.is_empty());
    }
}
