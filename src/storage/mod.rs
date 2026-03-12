pub mod identity;
pub mod peers;
pub mod records;
pub mod revocations;

use crate::error::MeshResult;
use rusqlite::Connection;

/// Top-level storage handle (will be expanded in later subtasks).
pub struct MeshStorage;

/// Check whether a given column already exists on a table.
fn column_exists(conn: &Connection, table: &str, column: &str) -> bool {
    let sql = format!("PRAGMA table_info({table})");
    let Ok(mut stmt) = conn.prepare(&sql) else {
        return false;
    };
    let Ok(rows) = stmt.query_map([], |row| row.get::<_, String>(1)) else {
        return false;
    };
    for name in rows.flatten() {
        if name == column {
            return true;
        }
    }
    false
}

/// Check whether a table exists in the database.
fn table_exists(conn: &Connection, table: &str) -> bool {
    let sql = "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1";
    conn.query_row(sql, [table], |row| row.get::<_, i64>(0))
        .unwrap_or(0)
        > 0
}

/// Run all mesh schema migrations. Each migration is idempotent.
pub fn run_migrations(conn: &Connection) -> MeshResult<()> {
    // Migration 1: Add visibility column to existing lessons/checkpoints tables.
    for table in &["lessons", "checkpoints"] {
        if table_exists(conn, table) && !column_exists(conn, table, "visibility") {
            conn.execute_batch(&format!(
                "ALTER TABLE {table} ADD COLUMN visibility TEXT NOT NULL DEFAULT 'private';"
            ))?;
        }
    }

    // Migration 2: mesh_identity table.
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS mesh_identity (
            id INTEGER PRIMARY KEY CHECK (id = 1),
            private_key BLOB NOT NULL,
            public_key BLOB NOT NULL,
            node_id TEXT NOT NULL,
            fingerprint TEXT NOT NULL,
            created_at INTEGER NOT NULL
        );",
    )?;

    // Migration 3: mesh_peers table.
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS mesh_peers (
            node_id TEXT PRIMARY KEY,
            endpoint TEXT NOT NULL,
            trust_level TEXT NOT NULL DEFAULT 'none',
            last_seen INTEGER,
            connected_since INTEGER,
            created_at INTEGER NOT NULL
        );",
    )?;

    // Migration 4: mesh_remote_records table with indexes.
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS mesh_remote_records (
            id TEXT PRIMARY KEY,
            record_type TEXT NOT NULL,
            publisher_node_id TEXT NOT NULL,
            signed_record TEXT NOT NULL,
            visibility TEXT NOT NULL,
            received_at INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_remote_records_publisher
            ON mesh_remote_records(publisher_node_id);
        CREATE INDEX IF NOT EXISTS idx_remote_records_type
            ON mesh_remote_records(record_type);",
    )?;

    // Migration 5: mesh_revocations table.
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS mesh_revocations (
            record_id TEXT PRIMARY KEY,
            record_type TEXT NOT NULL,
            publisher_node_id TEXT NOT NULL,
            revocation_json TEXT NOT NULL,
            revoked_at INTEGER NOT NULL,
            received_at INTEGER NOT NULL
        );",
    )?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    /// Helper: list table names created by migrations.
    fn mesh_tables(conn: &Connection) -> Vec<String> {
        let mut stmt = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name LIKE 'mesh_%' ORDER BY name")
            .unwrap();
        stmt.query_map([], |row| row.get(0))
            .unwrap()
            .flatten()
            .collect()
    }

    #[test]
    fn migrations_create_all_tables() {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();

        let tables = mesh_tables(&conn);
        assert!(tables.contains(&"mesh_identity".to_string()));
        assert!(tables.contains(&"mesh_peers".to_string()));
        assert!(tables.contains(&"mesh_remote_records".to_string()));
        assert!(tables.contains(&"mesh_revocations".to_string()));

        // Verify columns on mesh_identity via pragma.
        assert!(column_exists(&conn, "mesh_identity", "private_key"));
        assert!(column_exists(&conn, "mesh_identity", "public_key"));
        assert!(column_exists(&conn, "mesh_identity", "node_id"));
        assert!(column_exists(&conn, "mesh_identity", "fingerprint"));
        assert!(column_exists(&conn, "mesh_identity", "created_at"));

        // Verify columns on mesh_peers.
        assert!(column_exists(&conn, "mesh_peers", "node_id"));
        assert!(column_exists(&conn, "mesh_peers", "endpoint"));
        assert!(column_exists(&conn, "mesh_peers", "trust_level"));
        assert!(column_exists(&conn, "mesh_peers", "last_seen"));
        assert!(column_exists(&conn, "mesh_peers", "connected_since"));

        // Verify columns on mesh_remote_records.
        assert!(column_exists(
            &conn,
            "mesh_remote_records",
            "publisher_node_id"
        ));
        assert!(column_exists(&conn, "mesh_remote_records", "signed_record"));
        assert!(column_exists(&conn, "mesh_remote_records", "visibility"));

        // Verify columns on mesh_revocations.
        assert!(column_exists(&conn, "mesh_revocations", "record_id"));
        assert!(column_exists(&conn, "mesh_revocations", "revocation_json"));
        assert!(column_exists(&conn, "mesh_revocations", "revoked_at"));
        assert!(column_exists(&conn, "mesh_revocations", "received_at"));
    }

    #[test]
    fn migrations_are_idempotent() {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();
        run_migrations(&conn).unwrap(); // second run must not error
        let tables = mesh_tables(&conn);
        assert_eq!(tables.len(), 4);
    }

    #[test]
    fn migrations_add_visibility_to_existing_tables() {
        let conn = Connection::open_in_memory().unwrap();

        // Simulate pre-existing lessons and checkpoints tables without visibility.
        conn.execute_batch(
            "CREATE TABLE lessons (id INTEGER PRIMARY KEY, title TEXT NOT NULL);
             CREATE TABLE checkpoints (id INTEGER PRIMARY KEY, title TEXT NOT NULL);",
        )
        .unwrap();

        assert!(!column_exists(&conn, "lessons", "visibility"));
        assert!(!column_exists(&conn, "checkpoints", "visibility"));

        run_migrations(&conn).unwrap();

        assert!(column_exists(&conn, "lessons", "visibility"));
        assert!(column_exists(&conn, "checkpoints", "visibility"));
    }

    #[test]
    fn column_exists_works_correctly() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE test_tbl (col_a TEXT, col_b INTEGER);")
            .unwrap();

        assert!(column_exists(&conn, "test_tbl", "col_a"));
        assert!(column_exists(&conn, "test_tbl", "col_b"));
        assert!(!column_exists(&conn, "test_tbl", "col_c"));
        // Non-existent table returns false.
        assert!(!column_exists(&conn, "no_such_table", "col_a"));
    }

    #[test]
    fn mesh_identity_check_constraint() {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();

        // id=1 should succeed.
        conn.execute(
            "INSERT INTO mesh_identity (id, private_key, public_key, node_id, fingerprint, created_at)
             VALUES (1, X'AA', X'BB', 'node-1', 'fp-1', 1000)",
            [],
        )
        .unwrap();

        // id=2 should fail due to CHECK constraint.
        let result = conn.execute(
            "INSERT INTO mesh_identity (id, private_key, public_key, node_id, fingerprint, created_at)
             VALUES (2, X'AA', X'BB', 'node-2', 'fp-2', 1000)",
            [],
        );
        assert!(result.is_err(), "CHECK (id = 1) should reject id=2");
    }
}
