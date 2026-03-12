use std::sync::Arc;

use axum::extract::State;
use axum::response::IntoResponse;

use crate::error::MeshError;

use super::MeshState;

pub async fn get_identity(
    State(state): State<Arc<MeshState>>,
) -> Result<impl IntoResponse, MeshError> {
    let doc = state.identity.identity_document(&state.mesh_endpoint)?;
    Ok(axum::Json(doc))
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    use crate::http::{mesh_router, MeshState};
    use crate::identity::{IdentityDocument, NodeIdentity};
    use crate::storage::run_migrations;

    fn test_state() -> Arc<MeshState> {
        let identity = NodeIdentity::generate();
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();
        Arc::new(MeshState {
            identity: Arc::new(identity),
            db: Arc::new(Mutex::new(conn)),
            mesh_endpoint: "https://example.com/mesh".to_string(),
        })
    }

    #[tokio::test]
    async fn get_identity_returns_200_with_valid_json() {
        let state = test_state();
        let app = mesh_router(state.clone());

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/mesh/v1/identity")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let doc: IdentityDocument = serde_json::from_slice(&body).unwrap();
        assert_eq!(doc.version, "mesh/1.0");
    }

    #[tokio::test]
    async fn get_identity_contains_correct_node_id() {
        let state = test_state();
        let expected_node_id = state.identity.node_id().to_string();
        let app = mesh_router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/mesh/v1/identity")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let doc: IdentityDocument = serde_json::from_slice(&body).unwrap();
        assert_eq!(doc.node_id, expected_node_id);
    }

    #[tokio::test]
    async fn get_identity_self_signature_is_valid() {
        let state = test_state();
        let app = mesh_router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/mesh/v1/identity")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let doc: IdentityDocument = serde_json::from_slice(&body).unwrap();
        doc.verify().expect("self-signature should be valid");
    }

    #[tokio::test]
    async fn well_known_endpoint_returns_same_response() {
        let state = test_state();
        let expected_node_id = state.identity.node_id().to_string();
        let app = mesh_router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/.well-known/mesh/identity")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let doc: IdentityDocument = serde_json::from_slice(&body).unwrap();
        assert_eq!(doc.node_id, expected_node_id);
        assert_eq!(doc.version, "mesh/1.0");
        doc.verify().expect("self-signature should be valid");
    }
}
