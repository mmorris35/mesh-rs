use std::sync::{Arc, Mutex};

use chrono::Utc;
use rusqlite::Connection;
use tracing::{info, warn};

use crate::envelope::MeshEnvelope;
use crate::error::{MeshError, MeshResult};
use crate::identity::NodeIdentity;
use crate::signing::sign_revocation;
use crate::storage::MeshStorage;
use crate::types::*;

/// Handles record revocation and announcement to peers.
pub struct Revoker {
    identity: Arc<NodeIdentity>,
    db: Arc<Mutex<Connection>>,
    http_client: reqwest::Client,
}

impl Revoker {
    /// Create a new `Revoker`.
    pub fn new(
        identity: Arc<NodeIdentity>,
        db: Arc<Mutex<Connection>>,
        http_client: reqwest::Client,
    ) -> Self {
        Self {
            identity,
            db,
            http_client,
        }
    }

    /// Revoke a record by building and signing a `Revocation`.
    pub fn revoke(
        &self,
        record_id: &str,
        record_type: RecordType,
        reason: Option<&str>,
    ) -> MeshResult<Revocation> {
        let mut revocation = Revocation {
            record_type,
            record_id: record_id.to_string(),
            node_id: self.identity.node_id().to_string(),
            revoked_at: Utc::now().timestamp_millis(),
            reason: reason.map(|r| r.to_string()),
            signature: None,
        };

        sign_revocation(&self.identity, &mut revocation)?;

        info!(
            record_id = record_id,
            node_id = self.identity.node_id(),
            "Record revoked"
        );

        Ok(revocation)
    }

    /// Revoke a record and announce the revocation to all trusted peers.
    ///
    /// Returns the signed revocation and a list of `(peer_endpoint, result)` pairs
    /// indicating whether each peer was successfully notified.
    pub async fn revoke_and_announce(
        &self,
        record_id: &str,
        record_type: RecordType,
        reason: Option<&str>,
    ) -> MeshResult<(Revocation, Vec<(String, Result<(), MeshError>)>)> {
        let revocation = self.revoke(record_id, record_type, reason)?;

        let announcement = RevocationAnnouncement {
            revocation: revocation.clone(),
        };

        let payload = serde_json::to_value(&announcement)
            .map_err(|e| MeshError::SerializationError(e.to_string()))?;

        let mut envelope = MeshEnvelope::new(self.identity.node_id(), "revocation", payload);
        envelope.sign(&self.identity)?;

        let results = self.announce_to_peers(&envelope).await?;

        Ok((revocation, results))
    }

    /// Send an envelope to all trusted peers, collecting per-peer results.
    async fn announce_to_peers(
        &self,
        envelope: &MeshEnvelope,
    ) -> MeshResult<Vec<(String, Result<(), MeshError>)>> {
        let peers = {
            let conn = self
                .db
                .lock()
                .map_err(|e| MeshError::StorageError(format!("lock poisoned: {e}")))?;
            MeshStorage::get_trusted_peers(&conn)?
        };

        let mut results = Vec::new();

        for peer in &peers {
            let url = format!("{}/mesh/v1/announce", peer.endpoint.trim_end_matches('/'));
            let result = self
                .http_client
                .post(&url)
                .json(envelope)
                .send()
                .await
                .map_err(|e| MeshError::NetworkError(e.to_string()))
                .and_then(|resp| {
                    if resp.status().is_success() {
                        Ok(())
                    } else {
                        Err(MeshError::NetworkError(format!(
                            "peer {} returned status {}",
                            peer.node_id,
                            resp.status()
                        )))
                    }
                });

            match &result {
                Ok(()) => info!(peer = %peer.node_id, "Revocation announced to peer"),
                Err(e) => {
                    warn!(peer = %peer.node_id, error = %e, "Failed to announce revocation to peer")
                }
            }

            results.push((peer.endpoint.clone(), result));
        }

        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signing::verify_revocation;
    use crate::storage::run_migrations;

    fn setup() -> (Arc<NodeIdentity>, Arc<Mutex<Connection>>) {
        let identity = Arc::new(NodeIdentity::generate());
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();
        let db = Arc::new(Mutex::new(conn));
        (identity, db)
    }

    #[test]
    fn revoke_produces_valid_signed_revocation() {
        let (identity, db) = setup();
        let revoker = Revoker::new(identity, db, reqwest::Client::new());

        let revocation = revoker
            .revoke("lesson-42", RecordType::Lesson, Some("outdated"))
            .unwrap();

        // Signature must be present and valid
        assert!(revocation.signature.is_some());
        verify_revocation(&revocation).unwrap();
    }

    #[test]
    fn revocation_node_id_matches_identity() {
        let (identity, db) = setup();
        let expected_node_id = identity.node_id().to_string();
        let revoker = Revoker::new(identity, db, reqwest::Client::new());

        let revocation = revoker
            .revoke("cp-1", RecordType::Checkpoint, None)
            .unwrap();

        assert_eq!(revocation.node_id, expected_node_id);
    }

    #[test]
    fn revocation_timestamp_is_recent() {
        let (identity, db) = setup();
        let revoker = Revoker::new(identity, db, reqwest::Client::new());

        let before = Utc::now().timestamp_millis();
        let revocation = revoker
            .revoke("lesson-1", RecordType::Lesson, None)
            .unwrap();
        let after = Utc::now().timestamp_millis();

        assert!(revocation.revoked_at >= before);
        assert!(revocation.revoked_at <= after);
    }

    #[test]
    fn revocation_reason_is_preserved() {
        let (identity, db) = setup();
        let revoker = Revoker::new(identity, db, reqwest::Client::new());

        // With reason
        let rev_with = revoker
            .revoke("l-1", RecordType::Lesson, Some("contains errors"))
            .unwrap();
        assert_eq!(rev_with.reason.as_deref(), Some("contains errors"));

        // Without reason
        let rev_without = revoker.revoke("l-2", RecordType::Lesson, None).unwrap();
        assert!(rev_without.reason.is_none());
    }

    #[test]
    fn revoke_checkpoint_record_type() {
        let (identity, db) = setup();
        let revoker = Revoker::new(identity, db, reqwest::Client::new());

        let revocation = revoker
            .revoke("cp-5", RecordType::Checkpoint, Some("superseded"))
            .unwrap();

        assert_eq!(revocation.record_type, RecordType::Checkpoint);
        assert_eq!(revocation.record_id, "cp-5");
        assert_eq!(revocation.reason.as_deref(), Some("superseded"));
        verify_revocation(&revocation).unwrap();
    }
}
