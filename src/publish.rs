use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::Utc;
use rusqlite::Connection;
use tracing::{info, warn};

use crate::envelope::MeshEnvelope;
use crate::error::{MeshError, MeshResult};
use crate::identity::NodeIdentity;
use crate::signing::{sign_checkpoint, sign_lesson};
use crate::storage::MeshStorage;
use crate::types::*;

/// Handles publishing lessons and checkpoints, and announcing them to trusted peers.
pub struct Publisher {
    identity: Arc<NodeIdentity>,
    db: Arc<Mutex<Connection>>,
    http_client: reqwest::Client,
}

impl Publisher {
    /// Create a new `Publisher`.
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

    /// Sign and return a `SignedLesson`.
    ///
    /// Creates a `Publication` with the given visibility and topics, then signs
    /// the lesson using the node's Ed25519 identity.
    pub fn publish_lesson(
        &self,
        lesson: serde_json::Value,
        visibility: Visibility,
        topics: Option<Vec<String>>,
    ) -> MeshResult<SignedLesson> {
        let publication = Publication {
            visibility,
            published_at: Utc::now().timestamp_millis(),
            topics,
        };
        sign_lesson(&self.identity, &lesson, &publication)
    }

    /// Sign and return a `SignedCheckpoint`.
    ///
    /// Creates a `Publication` with the given visibility and topics, then signs
    /// the checkpoint using the node's Ed25519 identity.
    pub fn publish_checkpoint(
        &self,
        checkpoint: serde_json::Value,
        visibility: Visibility,
        topics: Option<Vec<String>>,
    ) -> MeshResult<SignedCheckpoint> {
        let publication = Publication {
            visibility,
            published_at: Utc::now().timestamp_millis(),
            topics,
        };
        sign_checkpoint(&self.identity, &checkpoint, &publication)
    }

    /// Announce an envelope to all trusted peers.
    ///
    /// POSTs the envelope JSON to each trusted peer's `/mesh/v1/announce` endpoint
    /// with a 10-second timeout. Collects results without failing if individual
    /// peers are unreachable.
    pub async fn announce_to_peers(
        &self,
        envelope: &MeshEnvelope,
    ) -> MeshResult<Vec<(String, Result<(), MeshError>)>> {
        let peers = {
            let conn = self
                .db
                .lock()
                .map_err(|e| MeshError::StorageError(e.to_string()))?;
            MeshStorage::get_trusted_peers(&conn)?
        };

        let mut results = Vec::new();

        for peer in &peers {
            let url = format!("{}/mesh/v1/announce", peer.endpoint.trim_end_matches('/'));
            let outcome = self
                .http_client
                .post(&url)
                .timeout(Duration::from_secs(10))
                .json(envelope)
                .send()
                .await;

            match outcome {
                Ok(resp) if resp.status().is_success() => {
                    info!(
                        peer_node_id = %peer.node_id,
                        "Successfully announced to peer"
                    );
                    results.push((peer.node_id.clone(), Ok(())));
                }
                Ok(resp) => {
                    let status = resp.status();
                    warn!(
                        peer_node_id = %peer.node_id,
                        status = %status,
                        "Peer returned non-success status"
                    );
                    results.push((
                        peer.node_id.clone(),
                        Err(MeshError::NetworkError(format!(
                            "peer {} returned status {}",
                            peer.node_id, status
                        ))),
                    ));
                }
                Err(e) => {
                    warn!(
                        peer_node_id = %peer.node_id,
                        error = %e,
                        "Failed to announce to peer"
                    );
                    results.push((
                        peer.node_id.clone(),
                        Err(MeshError::NetworkError(e.to_string())),
                    ));
                }
            }
        }

        Ok(results)
    }

    /// Publish a lesson and announce it to all trusted peers.
    ///
    /// Signs the lesson, wraps it in a `PublicationAnnouncement`, creates a
    /// `MeshEnvelope`, signs the envelope, and sends it to trusted peers.
    pub async fn publish_and_announce_lesson(
        &self,
        lesson: serde_json::Value,
        visibility: Visibility,
        topics: Option<Vec<String>>,
    ) -> MeshResult<(SignedLesson, Vec<(String, Result<(), MeshError>)>)> {
        let signed = self.publish_lesson(lesson, visibility, topics)?;

        let announcement = PublicationAnnouncement {
            record_type: RecordType::Lesson,
            signed_lesson: Some(signed.clone()),
            signed_checkpoint: None,
        };

        let payload = serde_json::to_value(&announcement)
            .map_err(|e| MeshError::SerializationError(e.to_string()))?;

        let mut envelope = MeshEnvelope::new(self.identity.node_id(), "publication", payload);
        envelope.sign(&self.identity)?;

        let results = self.announce_to_peers(&envelope).await?;
        Ok((signed, results))
    }

    /// Publish a checkpoint and announce it to all trusted peers.
    ///
    /// Signs the checkpoint, wraps it in a `PublicationAnnouncement`, creates a
    /// `MeshEnvelope`, signs the envelope, and sends it to trusted peers.
    pub async fn publish_and_announce_checkpoint(
        &self,
        checkpoint: serde_json::Value,
        visibility: Visibility,
        topics: Option<Vec<String>>,
    ) -> MeshResult<(SignedCheckpoint, Vec<(String, Result<(), MeshError>)>)> {
        let signed = self.publish_checkpoint(checkpoint, visibility, topics)?;

        let announcement = PublicationAnnouncement {
            record_type: RecordType::Checkpoint,
            signed_lesson: None,
            signed_checkpoint: Some(signed.clone()),
        };

        let payload = serde_json::to_value(&announcement)
            .map_err(|e| MeshError::SerializationError(e.to_string()))?;

        let mut envelope = MeshEnvelope::new(self.identity.node_id(), "publication", payload);
        envelope.sign(&self.identity)?;

        let results = self.announce_to_peers(&envelope).await?;
        Ok((signed, results))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signing::{verify_signed_checkpoint, verify_signed_lesson};
    use crate::storage::run_migrations;
    use serde_json::json;

    fn setup() -> (Arc<NodeIdentity>, Arc<Mutex<Connection>>) {
        let identity = Arc::new(NodeIdentity::generate());
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();
        let db = Arc::new(Mutex::new(conn));
        (identity, db)
    }

    #[test]
    fn publish_lesson_produces_valid_signed_lesson() {
        let (identity, db) = setup();
        let publisher = Publisher::new(identity, db, reqwest::Client::new());
        let lesson = json!({"id": "l1", "content": "Learn Rust ownership"});

        let signed = publisher
            .publish_lesson(
                lesson.clone(),
                Visibility::Public,
                Some(vec!["rust".into()]),
            )
            .unwrap();

        assert_eq!(signed.lesson, lesson);
        assert_eq!(signed.publication.visibility, Visibility::Public);
        assert_eq!(signed.publication.topics, Some(vec!["rust".to_string()]));
        assert_eq!(signed.signature.algorithm, "ed25519");

        // Verify the signature is valid
        verify_signed_lesson(&signed).unwrap();
    }

    #[test]
    fn publish_checkpoint_produces_valid_signed_checkpoint() {
        let (identity, db) = setup();
        let publisher = Publisher::new(identity, db, reqwest::Client::new());
        let checkpoint = json!({"id": "cp1", "score": 95});

        let signed = publisher
            .publish_checkpoint(
                checkpoint.clone(),
                Visibility::Unlisted,
                Some(vec!["math".into()]),
            )
            .unwrap();

        assert_eq!(signed.checkpoint, checkpoint);
        assert_eq!(signed.publication.visibility, Visibility::Unlisted);

        // Verify the signature is valid
        verify_signed_checkpoint(&signed).unwrap();
    }

    #[test]
    fn publish_lesson_visibility_is_set_correctly() {
        let (identity, db) = setup();
        let publisher = Publisher::new(identity, db, reqwest::Client::new());
        let lesson = json!({"id": "l1"});

        let private = publisher
            .publish_lesson(lesson.clone(), Visibility::Private, None)
            .unwrap();
        assert_eq!(private.publication.visibility, Visibility::Private);

        let public = publisher
            .publish_lesson(lesson.clone(), Visibility::Public, None)
            .unwrap();
        assert_eq!(public.publication.visibility, Visibility::Public);

        let unlisted = publisher
            .publish_lesson(lesson, Visibility::Unlisted, None)
            .unwrap();
        assert_eq!(unlisted.publication.visibility, Visibility::Unlisted);
    }

    #[test]
    fn publish_lesson_timestamp_is_recent() {
        let (identity, db) = setup();
        let publisher = Publisher::new(identity, db, reqwest::Client::new());
        let lesson = json!({"id": "l1"});

        let before = Utc::now().timestamp_millis();
        let signed = publisher
            .publish_lesson(lesson, Visibility::Public, None)
            .unwrap();
        let after = Utc::now().timestamp_millis();

        assert!(
            signed.publication.published_at >= before,
            "published_at should be >= test start time"
        );
        assert!(
            signed.publication.published_at <= after,
            "published_at should be <= test end time"
        );
    }
}
