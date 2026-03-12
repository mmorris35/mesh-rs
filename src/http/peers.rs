use std::sync::Arc;

use axum::extract::State;
use axum::response::IntoResponse;

use crate::error::MeshError;
use crate::storage::MeshStorage;

use super::MeshState;

pub async fn get_peers(
    State(state): State<Arc<MeshState>>,
) -> Result<impl IntoResponse, MeshError> {
    let conn = state
        .db
        .lock()
        .map_err(|e| MeshError::StorageError(e.to_string()))?;
    let peers = MeshStorage::list_peers(&conn)?;
    Ok(axum::Json(peers))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use std::sync::Mutex;
    use tower::ServiceExt;

    use crate::identity::NodeIdentity;
    use crate::storage::run_migrations;
    use crate::types::PeerConnection;

    fn test_state() -> Arc<MeshState> {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();
        let identity = NodeIdentity::generate();
        Arc::new(MeshState {
            identity: Arc::new(identity),
            db: Arc::new(Mutex::new(conn)),
            mesh_endpoint: "http://localhost:3000".to_string(),
        })
    }

    #[tokio::test]
    async fn get_peers_returns_empty_array() {
        let state = test_state();
        let app = crate::http::mesh_router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/mesh/v1/peers")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let peers: Vec<PeerConnection> = serde_json::from_slice(&body).unwrap();
        assert!(peers.is_empty());
    }

    #[tokio::test]
    async fn get_peers_returns_added_peers() {
        let state = test_state();

        // Add peers to DB
        {
            let conn = state.db.lock().unwrap();
            MeshStorage::add_peer(&conn, "node-1", "https://node1.example.com").unwrap();
            MeshStorage::add_peer(&conn, "node-2", "https://node2.example.com").unwrap();
        }

        let app = crate::http::mesh_router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/mesh/v1/peers")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let peers: Vec<PeerConnection> = serde_json::from_slice(&body).unwrap();
        assert_eq!(peers.len(), 2);

        let node_ids: Vec<&str> = peers.iter().map(|p| p.node_id.as_str()).collect();
        assert!(node_ids.contains(&"node-1"));
        assert!(node_ids.contains(&"node-2"));
    }
}
