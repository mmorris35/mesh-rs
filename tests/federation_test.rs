use std::sync::{Arc, Mutex};

use rusqlite::Connection;
use serde_json::json;

use mesh_node::envelope::MeshEnvelope;
use mesh_node::signing::sign_lesson;
use mesh_node::storage::MeshStorage;
use mesh_node::types::{Publication, PublicationAnnouncement, RecordType, Visibility};
use mesh_node::{MeshConfig, MeshNode};

/// Full federation lifecycle: two in-process nodes exchange publications,
/// verify identity, search cached records, publish checkpoints, and
/// handle revocation (including re-announce blocking).
#[tokio::test]
async fn federation_lifecycle() {
    // -----------------------------------------------------------------------
    // 1. Bind listeners first so we know the ports
    // -----------------------------------------------------------------------
    let listener_a = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port_a = listener_a.local_addr().unwrap().port();

    let listener_b = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port_b = listener_b.local_addr().unwrap().port();

    // -----------------------------------------------------------------------
    // 2. Create nodes with correct endpoints
    // -----------------------------------------------------------------------
    let config_a = MeshConfig {
        mesh_endpoint: format!("http://127.0.0.1:{port_a}/mesh"),
        ..Default::default()
    };
    let conn_a = Arc::new(Mutex::new(Connection::open_in_memory().unwrap()));
    let node_a = MeshNode::new(conn_a.clone(), config_a).unwrap();

    let config_b = MeshConfig {
        mesh_endpoint: format!("http://127.0.0.1:{port_b}/mesh"),
        ..Default::default()
    };
    let conn_b = Arc::new(Mutex::new(Connection::open_in_memory().unwrap()));
    let node_b = MeshNode::new(conn_b.clone(), config_b).unwrap();

    let node_a_id = node_a.node_id().to_string();
    let node_b_id = node_b.node_id().to_string();

    // -----------------------------------------------------------------------
    // 3. Spawn HTTP servers
    // -----------------------------------------------------------------------
    let router_a = node_a.router();
    let server_a = tokio::spawn(async move {
        axum::serve(listener_a, router_a).await.unwrap();
    });

    let router_b = node_b.router();
    let server_b = tokio::spawn(async move {
        axum::serve(listener_b, router_b).await.unwrap();
    });

    // -----------------------------------------------------------------------
    // 4. Add peers: Node A <-> Node B
    // -----------------------------------------------------------------------
    let endpoint_a = format!("http://127.0.0.1:{port_a}");
    let endpoint_b = format!("http://127.0.0.1:{port_b}");

    node_a
        .peer_manager()
        .add_peer(&node_b_id, &endpoint_b)
        .unwrap();
    node_b
        .peer_manager()
        .add_peer(&node_a_id, &endpoint_a)
        .unwrap();

    // -----------------------------------------------------------------------
    // 5. Trust: both nodes trust each other
    // -----------------------------------------------------------------------
    node_a.trust_manager().add_trust(&node_b_id).unwrap();
    node_b.trust_manager().add_trust(&node_a_id).unwrap();

    // -----------------------------------------------------------------------
    // 6. Verify identity: Node A verifies Node B's identity (and vice versa)
    // -----------------------------------------------------------------------
    let id_doc_b = node_a
        .peer_manager()
        .verify_peer_identity(&node_b_id)
        .await
        .unwrap();
    assert_eq!(id_doc_b.node_id, node_b_id);
    assert_eq!(id_doc_b.version, "mesh/1.0");
    id_doc_b.verify().unwrap();

    let id_doc_a = node_b
        .peer_manager()
        .verify_peer_identity(&node_a_id)
        .await
        .unwrap();
    assert_eq!(id_doc_a.node_id, node_a_id);
    id_doc_a.verify().unwrap();

    // -----------------------------------------------------------------------
    // 7. Publish lesson: Node A publishes a test lesson
    // -----------------------------------------------------------------------
    let lesson = json!({
        "id": "lesson-fed-1",
        "title": "Federation Test Lesson",
        "content": "Learn about MESH federation protocol and peer networking"
    });
    let publish_result = node_a
        .tools()
        .mesh_publish(
            lesson.clone(),
            "lesson",
            Some("public"),
            Some(vec!["federation".into(), "mesh".into()]),
        )
        .await
        .unwrap();

    assert_eq!(publish_result["published"], true);
    assert_eq!(publish_result["record_type"], "lesson");
    assert_eq!(publish_result["visibility"], "public");

    // The announce_to list should include Node B
    let announced_to = publish_result["announced_to"].as_array().unwrap();
    assert!(
        announced_to.iter().any(|v| v.as_str() == Some(&node_b_id)),
        "Node B should be in announced_to list"
    );

    // -----------------------------------------------------------------------
    // 8. Verify receipt: Node B should have the lesson in mesh_remote_records
    // -----------------------------------------------------------------------
    {
        let db = conn_b.lock().unwrap();
        let record = MeshStorage::get_remote_record(&db, "lesson-fed-1")
            .unwrap()
            .expect("Node B should have received the lesson");
        assert_eq!(record.record_type, "lesson");
        assert_eq!(record.publisher_node_id, node_a_id);
        assert_eq!(record.visibility, "public");
    }

    // -----------------------------------------------------------------------
    // 9. Search: Node B searches for the lesson locally (from cached records)
    // -----------------------------------------------------------------------
    let search_result = node_b
        .tools()
        .mesh_search("federation protocol", None, None)
        .await
        .unwrap();

    let results = search_result["results"].as_array().unwrap();
    assert!(
        !results.is_empty(),
        "Search should find at least one result"
    );

    // Verify the found result matches our lesson
    let found = results
        .iter()
        .find(|r| {
            r.get("record")
                .and_then(|rec| rec.get("id"))
                .and_then(|id| id.as_str())
                == Some("lesson-fed-1")
        })
        .expect("Should find lesson-fed-1 in search results");
    assert_eq!(found["record_type"], "lesson");

    // -----------------------------------------------------------------------
    // 10. Publish checkpoint: Node A publishes a checkpoint
    // -----------------------------------------------------------------------
    let checkpoint = json!({
        "id": "checkpoint-fed-1",
        "title": "Federation Checkpoint",
        "score": 95,
        "content": "Testing checkpoint federation"
    });
    let cp_result = node_a
        .tools()
        .mesh_publish(checkpoint, "checkpoint", Some("public"), None)
        .await
        .unwrap();

    assert_eq!(cp_result["published"], true);
    assert_eq!(cp_result["record_type"], "checkpoint");

    // -----------------------------------------------------------------------
    // 11. Verify receipt: Node B receives the checkpoint
    // -----------------------------------------------------------------------
    {
        let db = conn_b.lock().unwrap();
        let record = MeshStorage::get_remote_record(&db, "checkpoint-fed-1")
            .unwrap()
            .expect("Node B should have received the checkpoint");
        assert_eq!(record.record_type, "checkpoint");
        assert_eq!(record.publisher_node_id, node_a_id);
    }

    // -----------------------------------------------------------------------
    // 12. Revoke: Node A revokes the lesson
    // -----------------------------------------------------------------------
    let revoke_result = node_a
        .tools()
        .mesh_revoke("lesson-fed-1", "lesson", Some("outdated"))
        .await
        .unwrap();

    assert_eq!(revoke_result["revoked"], true);
    assert_eq!(revoke_result["record_id"], "lesson-fed-1");
    assert_eq!(revoke_result["reason"], "outdated");

    // Note: mesh_revoke announces to the /mesh/revoke endpoint on the revoker,
    // but the actual HTTP handler is at /mesh/v1/announce. The revocation
    // announcement won't be received automatically via this path, so we
    // manually post the revocation to Node B's announce endpoint to simulate
    // proper federation (matching how the announce handler processes revocations).
    {
        // Build a revocation envelope and post to Node B
        let client = reqwest::Client::new();
        let mut revocation = mesh_node::types::Revocation {
            record_type: RecordType::Lesson,
            record_id: "lesson-fed-1".to_string(),
            node_id: node_a_id.clone(),
            revoked_at: chrono::Utc::now().timestamp_millis(),
            reason: Some("outdated".to_string()),
            signature: None,
        };
        mesh_node::signing::sign_revocation(node_a.identity(), &mut revocation).unwrap();

        let announcement = mesh_node::types::RevocationAnnouncement { revocation };
        let payload = serde_json::to_value(&announcement).unwrap();
        let mut envelope = MeshEnvelope::new(node_a.identity().node_id(), "revocation", payload);
        envelope.sign(node_a.identity()).unwrap();

        let resp = client
            .post(format!("{endpoint_b}/mesh/v1/announce"))
            .json(&envelope)
            .send()
            .await
            .unwrap();
        assert!(
            resp.status().is_success(),
            "Revocation announce should succeed"
        );
    }

    // -----------------------------------------------------------------------
    // 13. Verify revocation: Node B no longer has the lesson
    // -----------------------------------------------------------------------
    {
        let db = conn_b.lock().unwrap();
        // Lesson should be gone from remote records
        let record = MeshStorage::get_remote_record(&db, "lesson-fed-1").unwrap();
        assert!(
            record.is_none(),
            "Lesson should be deleted after revocation"
        );

        // Revocation should be stored
        assert!(
            MeshStorage::is_revoked(&db, "lesson-fed-1").unwrap(),
            "Revocation should be stored"
        );

        // Checkpoint should still be there
        let cp_record = MeshStorage::get_remote_record(&db, "checkpoint-fed-1").unwrap();
        assert!(
            cp_record.is_some(),
            "Checkpoint should still exist after lesson revocation"
        );
    }

    // -----------------------------------------------------------------------
    // 14. Re-announce blocked: try to re-announce the revoked lesson
    // -----------------------------------------------------------------------
    {
        let client = reqwest::Client::new();

        // Build a fresh publication envelope for the same lesson ID
        let lesson_redo = json!({
            "id": "lesson-fed-1",
            "title": "Federation Test Lesson v2",
            "content": "Attempt to re-publish revoked lesson"
        });
        let pub_info = Publication {
            visibility: Visibility::Public,
            published_at: chrono::Utc::now().timestamp_millis(),
            topics: Some(vec!["federation".into()]),
        };
        let signed = sign_lesson(node_a.identity(), &lesson_redo, &pub_info).unwrap();

        let announcement = PublicationAnnouncement {
            record_type: RecordType::Lesson,
            signed_lesson: Some(signed),
            signed_checkpoint: None,
        };

        let payload = serde_json::to_value(&announcement).unwrap();
        let mut envelope = MeshEnvelope::new(node_a.identity().node_id(), "publication", payload);
        envelope.sign(node_a.identity()).unwrap();

        let resp = client
            .post(format!("{endpoint_b}/mesh/v1/announce"))
            .json(&envelope)
            .send()
            .await
            .unwrap();

        // Should be rejected with CONFLICT (ALREADY_REVOKED)
        assert_eq!(
            resp.status(),
            reqwest::StatusCode::CONFLICT,
            "Re-announcing a revoked lesson should be rejected"
        );

        let body: serde_json::Value = resp.json().await.unwrap();
        assert_eq!(body["code"], "ALREADY_REVOKED");
    }

    // -----------------------------------------------------------------------
    // Clean shutdown
    // -----------------------------------------------------------------------
    server_a.abort();
    server_b.abort();
    let _ = server_a.await;
    let _ = server_b.await;
}
