use std::sync::Arc;

use axum::extract::State;
use axum::response::IntoResponse;
use axum::Json;
use chrono::Utc;
use ed25519_dalek::VerifyingKey;

use crate::envelope::MeshEnvelope;
use crate::error::MeshError;
use crate::signing::{verify_revocation, verify_signed_checkpoint, verify_signed_lesson};
use crate::storage::MeshStorage;
use crate::types::{PublicationAnnouncement, RecordType, RevocationAnnouncement, TrustLevel};

use super::MeshState;

/// Maximum allowed clock skew for envelope timestamps (±1 hour).
const TIMESTAMP_WINDOW_MS: i64 = 3_600_000;

pub async fn post_announce(
    State(state): State<Arc<MeshState>>,
    Json(envelope): Json<MeshEnvelope>,
) -> Result<impl IntoResponse, MeshError> {
    let conn = state
        .db
        .lock()
        .map_err(|e| MeshError::StorageError(e.to_string()))?;

    // 1. Check sender is a known peer
    let peer = MeshStorage::get_peer(&conn, &envelope.sender)?
        .ok_or_else(|| MeshError::UntrustedNode(envelope.sender.clone()))?;

    // 2. Check sender is trusted
    if peer.trust_level != TrustLevel::Full {
        return Err(MeshError::UntrustedNode(envelope.sender.clone()));
    }

    // 3. Verify envelope signature using sender's public key (derived from node_id)
    let pk_bytes = bs58::decode(&peer.node_id)
        .into_vec()
        .map_err(|_| MeshError::InvalidSignature)?;
    let pk_array: [u8; 32] = pk_bytes
        .try_into()
        .map_err(|_| MeshError::InvalidSignature)?;
    let verifying_key =
        VerifyingKey::from_bytes(&pk_array).map_err(|_| MeshError::InvalidSignature)?;
    envelope.verify_signature_with_key(&verifying_key)?;

    // 4. Validate envelope timestamp is within acceptable window (±1 hour)
    let now = Utc::now().timestamp_millis();
    let drift = (envelope.timestamp - now).abs();
    if drift > TIMESTAMP_WINDOW_MS {
        return Err(MeshError::InvalidRequest(format!(
            "envelope timestamp too far from current time (drift: {}ms)",
            drift
        )));
    }

    // 5. Process based on message type
    match envelope.msg_type.as_str() {
        "publication" => {
            let announcement: PublicationAnnouncement =
                serde_json::from_value(envelope.payload.clone())?;
            process_publication(&conn, &announcement)?;
        }
        "revocation" => {
            let announcement: RevocationAnnouncement =
                serde_json::from_value(envelope.payload.clone())?;
            process_revocation(&conn, &announcement, &envelope.sender)?;
        }
        _ => {
            return Err(MeshError::InvalidRequest(format!(
                "unknown message type: {}",
                envelope.msg_type
            )));
        }
    }

    Ok(Json(serde_json::json!({"accepted": true})))
}

fn process_publication(
    conn: &rusqlite::Connection,
    announcement: &PublicationAnnouncement,
) -> Result<(), MeshError> {
    match announcement.record_type {
        RecordType::Lesson => {
            let signed_lesson = announcement
                .signed_lesson
                .as_ref()
                .ok_or_else(|| MeshError::InvalidRequest("missing signedLesson".to_string()))?;

            // Verify signature
            verify_signed_lesson(signed_lesson)?;

            // Extract record ID
            let record_id = signed_lesson.lesson["id"]
                .as_str()
                .ok_or_else(|| MeshError::InvalidRequest("lesson missing id field".to_string()))?;

            // Check not already revoked
            if MeshStorage::is_revoked(conn, record_id)? {
                return Err(MeshError::AlreadyRevoked(record_id.to_string()));
            }

            // Serialize the signed record for storage
            let signed_record_json = serde_json::to_string(signed_lesson)?;
            let visibility = serde_json::to_value(&signed_lesson.publication.visibility)?;
            let visibility_str = visibility.as_str().unwrap_or("private");

            // Store the remote record
            MeshStorage::store_remote_record(
                conn,
                record_id,
                "lesson",
                &signed_lesson.signature.node_id,
                &signed_record_json,
                visibility_str,
            )?;
        }
        RecordType::Checkpoint => {
            let signed_checkpoint = announcement
                .signed_checkpoint
                .as_ref()
                .ok_or_else(|| MeshError::InvalidRequest("missing signedCheckpoint".to_string()))?;

            // Verify signature
            verify_signed_checkpoint(signed_checkpoint)?;

            // Extract record ID
            let record_id = signed_checkpoint.checkpoint["id"].as_str().ok_or_else(|| {
                MeshError::InvalidRequest("checkpoint missing id field".to_string())
            })?;

            // Check not already revoked
            if MeshStorage::is_revoked(conn, record_id)? {
                return Err(MeshError::AlreadyRevoked(record_id.to_string()));
            }

            // Serialize the signed record for storage
            let signed_record_json = serde_json::to_string(signed_checkpoint)?;
            let visibility = serde_json::to_value(&signed_checkpoint.publication.visibility)?;
            let visibility_str = visibility.as_str().unwrap_or("private");

            // Store the remote record
            MeshStorage::store_remote_record(
                conn,
                record_id,
                "checkpoint",
                &signed_checkpoint.signature.node_id,
                &signed_record_json,
                visibility_str,
            )?;
        }
    }

    Ok(())
}

fn process_revocation(
    conn: &rusqlite::Connection,
    announcement: &RevocationAnnouncement,
    sender: &str,
) -> Result<(), MeshError> {
    let revocation = &announcement.revocation;

    // Verify revocation signature
    verify_revocation(revocation)?;

    // Check revoker matches: revocation.node_id should match sender or original publisher
    let sig_block = revocation
        .signature
        .as_ref()
        .ok_or(MeshError::InvalidSignature)?;

    if sig_block.node_id != sender && revocation.node_id != sender {
        return Err(MeshError::UntrustedNode(format!(
            "revocation signer does not match sender: {}",
            sender
        )));
    }

    let record_type_str = match revocation.record_type {
        RecordType::Lesson => "lesson",
        RecordType::Checkpoint => "checkpoint",
    };

    let revocation_json = serde_json::to_string(revocation)?;

    // Store revocation (this also deletes the remote record if it exists)
    MeshStorage::store_revocation(
        conn,
        &revocation.record_id,
        record_type_str,
        &revocation.node_id,
        &revocation_json,
    )?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::envelope::MeshEnvelope;
    use crate::http::{mesh_router, MeshState};
    use crate::identity::NodeIdentity;
    use crate::signing::{sign_lesson, sign_revocation};
    use crate::storage::run_migrations;
    use crate::types::{Publication, Revocation, TrustLevel, Visibility};

    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use rusqlite::Connection;
    use std::sync::{Arc, Mutex};
    use tower::ServiceExt;

    fn setup_state(receiver: &NodeIdentity) -> Arc<MeshState> {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();
        Arc::new(MeshState {
            identity: Arc::new(
                NodeIdentity::from_private_key_bytes(receiver.private_key_bytes()).unwrap(),
            ),
            db: Arc::new(Mutex::new(conn)),
            mesh_endpoint: "https://receiver.example.com/mesh".to_string(),
        })
    }

    fn add_trusted_peer(state: &MeshState, node_id: &str) {
        let conn = state.db.lock().unwrap();
        MeshStorage::add_peer(&conn, node_id, "https://publisher.example.com/mesh").unwrap();
        MeshStorage::set_trust(&conn, node_id, TrustLevel::Full).unwrap();
    }

    fn test_publication() -> Publication {
        Publication {
            visibility: Visibility::Public,
            published_at: 1_700_000_000_000,
            topics: Some(vec!["rust".to_string()]),
        }
    }

    fn build_publication_envelope(publisher: &NodeIdentity, lesson_id: &str) -> MeshEnvelope {
        let lesson = serde_json::json!({"id": lesson_id, "content": "Learn Rust ownership"});
        let pub_info = test_publication();
        let signed = sign_lesson(publisher, &lesson, &pub_info).unwrap();

        let announcement = PublicationAnnouncement {
            record_type: RecordType::Lesson,
            signed_lesson: Some(signed),
            signed_checkpoint: None,
        };

        let payload = serde_json::to_value(&announcement).unwrap();
        let mut envelope = MeshEnvelope::new(publisher.node_id(), "publication", payload);
        envelope.sign(publisher).unwrap();
        envelope
    }

    fn build_revocation_envelope(publisher: &NodeIdentity, record_id: &str) -> MeshEnvelope {
        let mut revocation = Revocation {
            record_type: RecordType::Lesson,
            record_id: record_id.to_string(),
            node_id: publisher.node_id().to_string(),
            revoked_at: chrono::Utc::now().timestamp_millis(),
            reason: Some("outdated".to_string()),
            signature: None,
        };
        sign_revocation(publisher, &mut revocation).unwrap();

        let announcement = RevocationAnnouncement { revocation };
        let payload = serde_json::to_value(&announcement).unwrap();
        let mut envelope = MeshEnvelope::new(publisher.node_id(), "revocation", payload);
        envelope.sign(publisher).unwrap();
        envelope
    }

    async fn send_announce(
        state: Arc<MeshState>,
        envelope: &MeshEnvelope,
    ) -> (StatusCode, serde_json::Value) {
        let router = mesh_router(state);
        let body = serde_json::to_string(envelope).unwrap();
        let request = Request::builder()
            .method("POST")
            .uri("/mesh/v1/announce")
            .header("content-type", "application/json")
            .body(Body::from(body))
            .unwrap();

        let response = router.oneshot(request).await.unwrap();
        let status = response.status();
        let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
        (status, json)
    }

    #[tokio::test]
    async fn valid_publication_from_trusted_peer_accepted() {
        let publisher = NodeIdentity::generate();
        let receiver = NodeIdentity::generate();
        let state = setup_state(&receiver);

        add_trusted_peer(&state, publisher.node_id());

        let envelope = build_publication_envelope(&publisher, "lesson-1");
        let (status, json) = send_announce(state.clone(), &envelope).await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["accepted"], true);

        // Verify record was stored
        let conn = state.db.lock().unwrap();
        let record = MeshStorage::get_remote_record(&conn, "lesson-1")
            .unwrap()
            .expect("record should be stored");
        assert_eq!(record.record_type, "lesson");
        assert_eq!(record.publisher_node_id, publisher.node_id());
        assert_eq!(record.visibility, "public");
    }

    #[tokio::test]
    async fn publication_from_unknown_peer_rejected() {
        let publisher = NodeIdentity::generate();
        let receiver = NodeIdentity::generate();
        let state = setup_state(&receiver);

        // Do NOT add publisher as a peer
        let envelope = build_publication_envelope(&publisher, "lesson-1");
        let (status, json) = send_announce(state, &envelope).await;

        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(json["code"], "UNTRUSTED_NODE");
    }

    #[tokio::test]
    async fn publication_from_untrusted_peer_rejected() {
        let publisher = NodeIdentity::generate();
        let receiver = NodeIdentity::generate();
        let state = setup_state(&receiver);

        // Add peer but do NOT trust them
        {
            let conn = state.db.lock().unwrap();
            MeshStorage::add_peer(&conn, publisher.node_id(), "https://publisher.example.com")
                .unwrap();
        }

        let envelope = build_publication_envelope(&publisher, "lesson-1");
        let (status, json) = send_announce(state, &envelope).await;

        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(json["code"], "UNTRUSTED_NODE");
    }

    #[tokio::test]
    async fn publication_with_invalid_signature_rejected() {
        let publisher = NodeIdentity::generate();
        let other = NodeIdentity::generate();
        let receiver = NodeIdentity::generate();
        let state = setup_state(&receiver);

        add_trusted_peer(&state, publisher.node_id());

        // Sign the lesson with a different identity so the record signature is invalid
        let lesson = serde_json::json!({"id": "lesson-bad", "content": "test"});
        let pub_info = test_publication();
        let mut signed = sign_lesson(&other, &lesson, &pub_info).unwrap();
        // Change the node_id on the signature to make it look like it came from publisher
        // but the actual crypto signature was made with `other`'s key, so verification fails
        // Actually, let's just tamper with the lesson content after signing
        signed.lesson = serde_json::json!({"id": "lesson-bad", "content": "tampered"});

        let announcement = PublicationAnnouncement {
            record_type: RecordType::Lesson,
            signed_lesson: Some(signed),
            signed_checkpoint: None,
        };

        let payload = serde_json::to_value(&announcement).unwrap();
        let mut envelope = MeshEnvelope::new(publisher.node_id(), "publication", payload);
        envelope.sign(&publisher).unwrap();

        let (status, json) = send_announce(state, &envelope).await;

        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(json["code"], "INVALID_SIGNATURE");
    }

    #[tokio::test]
    async fn revocation_deletes_existing_record() {
        let publisher = NodeIdentity::generate();
        let receiver = NodeIdentity::generate();
        let state = setup_state(&receiver);

        add_trusted_peer(&state, publisher.node_id());

        // First, publish a record
        let pub_envelope = build_publication_envelope(&publisher, "lesson-rev");
        let (status, _) = send_announce(state.clone(), &pub_envelope).await;
        assert_eq!(status, StatusCode::OK);

        // Verify it exists
        {
            let conn = state.db.lock().unwrap();
            assert!(MeshStorage::get_remote_record(&conn, "lesson-rev")
                .unwrap()
                .is_some());
        }

        // Now revoke it
        let rev_envelope = build_revocation_envelope(&publisher, "lesson-rev");
        let (status, json) = send_announce(state.clone(), &rev_envelope).await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["accepted"], true);

        // Verify record was deleted and revocation stored
        let conn = state.db.lock().unwrap();
        assert!(MeshStorage::get_remote_record(&conn, "lesson-rev")
            .unwrap()
            .is_none());
        assert!(MeshStorage::is_revoked(&conn, "lesson-rev").unwrap());
    }

    #[tokio::test]
    async fn duplicate_publication_of_revoked_record_rejected() {
        let publisher = NodeIdentity::generate();
        let receiver = NodeIdentity::generate();
        let state = setup_state(&receiver);

        add_trusted_peer(&state, publisher.node_id());

        // Publish, then revoke
        let pub_envelope = build_publication_envelope(&publisher, "lesson-dup");
        let (status, _) = send_announce(state.clone(), &pub_envelope).await;
        assert_eq!(status, StatusCode::OK);

        let rev_envelope = build_revocation_envelope(&publisher, "lesson-dup");
        let (status, _) = send_announce(state.clone(), &rev_envelope).await;
        assert_eq!(status, StatusCode::OK);

        // Try to publish again - should be rejected
        let pub_envelope2 = build_publication_envelope(&publisher, "lesson-dup");
        let (status, json) = send_announce(state, &pub_envelope2).await;

        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(json["code"], "ALREADY_REVOKED");
    }
}
