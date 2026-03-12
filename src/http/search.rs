use std::sync::Arc;

use axum::extract::State;
use axum::response::IntoResponse;
use axum::Json;

use super::MeshState;
use crate::envelope::MeshEnvelope;
use crate::error::MeshError;
use crate::storage::MeshStorage;
use crate::types::{
    RecordType, SearchRequest, SearchResponse, SearchResult, SignedCheckpoint, SignedLesson,
};

pub async fn post_search(
    State(state): State<Arc<MeshState>>,
    Json(envelope): Json<MeshEnvelope>,
) -> Result<impl IntoResponse, MeshError> {
    // 1. Parse the SearchRequest from envelope payload
    let request: SearchRequest = serde_json::from_value(envelope.payload)?;

    // 2. Clamp limit to max 100, default 20
    let limit = request.limit.unwrap_or(20).min(100);

    // 3. Determine record_type filter
    let record_type_filter = request.record_types.as_ref().and_then(|types| {
        if types.len() == 1 {
            match types[0] {
                RecordType::Lesson => Some("lesson"),
                RecordType::Checkpoint => Some("checkpoint"),
            }
        } else {
            None
        }
    });

    // 4. Search local records
    let conn = state
        .db
        .lock()
        .map_err(|e| MeshError::StorageError(e.to_string()))?;
    let records = MeshStorage::search_remote_records(
        &conn,
        &request.query,
        record_type_filter,
        (limit + 1) as i64,
    )?;

    // 5. Build results
    let truncated = records.len() > limit;
    let results: Vec<SearchResult> = records
        .into_iter()
        .take(limit)
        .filter_map(|record| {
            // Parse the signed record JSON
            let value: serde_json::Value = serde_json::from_str(&record.signed_record).ok()?;

            // Calculate basic text relevance score (simple word match)
            let score = calculate_text_score(&request.query, &record.signed_record);

            match record.record_type.as_str() {
                "lesson" => {
                    let signed_lesson: SignedLesson = serde_json::from_value(value).ok()?;
                    Some(SearchResult {
                        record_type: RecordType::Lesson,
                        signed_lesson: Some(signed_lesson),
                        signed_checkpoint: None,
                        score,
                        trust_score: 1.0,
                        via: None,
                    })
                }
                "checkpoint" => {
                    let signed_checkpoint: SignedCheckpoint = serde_json::from_value(value).ok()?;
                    Some(SearchResult {
                        record_type: RecordType::Checkpoint,
                        signed_lesson: None,
                        signed_checkpoint: Some(signed_checkpoint),
                        score,
                        trust_score: 1.0,
                        via: None,
                    })
                }
                _ => None,
            }
        })
        .collect();

    // 6. Build response
    let response = SearchResponse {
        request_id: request.request_id,
        results,
        truncated,
    };

    Ok(Json(response))
}

/// Simple text relevance scoring (word match ratio)
fn calculate_text_score(query: &str, content: &str) -> f64 {
    let query_words: Vec<&str> = query.split_whitespace().collect();
    if query_words.is_empty() {
        return 0.0;
    }
    let content_lower = content.to_lowercase();
    let matches = query_words
        .iter()
        .filter(|w| content_lower.contains(&w.to_lowercase()))
        .count();
    matches as f64 / query_words.len() as f64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::envelope::MeshEnvelope;
    use crate::storage::run_migrations;
    use crate::types::{Publication, SignatureBlock, Visibility};
    use axum::body::Body;
    use axum::http::Request;
    use axum::Router;
    use http_body_util::BodyExt;
    use rusqlite::Connection;
    use std::sync::Mutex;
    use tower::ServiceExt;

    fn sample_signature_block() -> SignatureBlock {
        SignatureBlock {
            algorithm: "ed25519".to_string(),
            node_id: "node-abc123".to_string(),
            public_key: "dGVzdHB1YmtleQ==".to_string(),
            timestamp: 1700000000000,
            sig: "dGVzdHNpZw==".to_string(),
        }
    }

    fn sample_publication() -> Publication {
        Publication {
            visibility: Visibility::Public,
            published_at: 1700000000000,
            topics: Some(vec!["math".to_string()]),
        }
    }

    fn make_signed_lesson_json(id: &str, title: &str) -> String {
        let lesson = SignedLesson {
            lesson: serde_json::json!({"id": id, "title": title}),
            publication: sample_publication(),
            signature: sample_signature_block(),
        };
        serde_json::to_string(&lesson).unwrap()
    }

    fn make_signed_checkpoint_json(id: &str, title: &str) -> String {
        let checkpoint = SignedCheckpoint {
            checkpoint: serde_json::json!({"id": id, "title": title}),
            publication: sample_publication(),
            signature: sample_signature_block(),
        };
        serde_json::to_string(&checkpoint).unwrap()
    }

    fn setup_state() -> Arc<MeshState> {
        let identity = Arc::new(crate::identity::NodeIdentity::generate());
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();
        Arc::new(MeshState {
            identity,
            db: Arc::new(Mutex::new(conn)),
            mesh_endpoint: "http://localhost:4200".to_string(),
        })
    }

    fn search_router(state: Arc<MeshState>) -> Router {
        Router::new()
            .route("/mesh/v1/search", axum::routing::post(post_search))
            .with_state(state)
    }

    fn make_search_envelope(
        query: &str,
        record_types: Option<Vec<RecordType>>,
        limit: Option<usize>,
    ) -> MeshEnvelope {
        let request = SearchRequest {
            query: query.to_string(),
            record_types,
            filters: None,
            limit,
            request_id: "req-test-1".to_string(),
            origin: "node-origin".to_string(),
        };
        MeshEnvelope::new(
            "node-origin",
            "search",
            serde_json::to_value(&request).unwrap(),
        )
    }

    async fn do_search(
        app: &Router,
        envelope: &MeshEnvelope,
    ) -> (axum::http::StatusCode, serde_json::Value) {
        let body = serde_json::to_string(envelope).unwrap();
        let req = Request::builder()
            .method("POST")
            .uri("/mesh/v1/search")
            .header("content-type", "application/json")
            .body(Body::from(body))
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        let status = resp.status();
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        (status, value)
    }

    #[tokio::test]
    async fn search_returns_matching_results() {
        let state = setup_state();
        {
            let conn = state.db.lock().unwrap();
            MeshStorage::store_remote_record(
                &conn,
                "lesson-1",
                "lesson",
                "node-pub",
                &make_signed_lesson_json("lesson-1", "Intro to Rust"),
                "public",
            )
            .unwrap();
            MeshStorage::store_remote_record(
                &conn,
                "lesson-2",
                "lesson",
                "node-pub",
                &make_signed_lesson_json("lesson-2", "Advanced Python"),
                "public",
            )
            .unwrap();
            MeshStorage::store_remote_record(
                &conn,
                "cp-1",
                "checkpoint",
                "node-pub",
                &make_signed_checkpoint_json("cp-1", "Rust Quiz"),
                "public",
            )
            .unwrap();
        }

        let app = search_router(state);
        let envelope = make_search_envelope("Rust", None, None);
        let (status, body) = do_search(&app, &envelope).await;

        assert_eq!(status, axum::http::StatusCode::OK);
        let resp: SearchResponse = serde_json::from_value(body).unwrap();
        assert_eq!(resp.request_id, "req-test-1");
        assert_eq!(resp.results.len(), 2);
        assert!(!resp.truncated);
    }

    #[tokio::test]
    async fn search_with_record_type_filter() {
        let state = setup_state();
        {
            let conn = state.db.lock().unwrap();
            MeshStorage::store_remote_record(
                &conn,
                "lesson-1",
                "lesson",
                "node-pub",
                &make_signed_lesson_json("lesson-1", "Intro to Rust"),
                "public",
            )
            .unwrap();
            MeshStorage::store_remote_record(
                &conn,
                "cp-1",
                "checkpoint",
                "node-pub",
                &make_signed_checkpoint_json("cp-1", "Rust Quiz"),
                "public",
            )
            .unwrap();
        }

        let app = search_router(state);

        // Filter to lessons only
        let envelope = make_search_envelope("Rust", Some(vec![RecordType::Lesson]), None);
        let (status, body) = do_search(&app, &envelope).await;
        assert_eq!(status, axum::http::StatusCode::OK);
        let resp: SearchResponse = serde_json::from_value(body).unwrap();
        assert_eq!(resp.results.len(), 1);
        assert_eq!(resp.results[0].record_type, RecordType::Lesson);

        // Filter to checkpoints only
        let envelope = make_search_envelope("Rust", Some(vec![RecordType::Checkpoint]), None);
        let (status, body) = do_search(&app, &envelope).await;
        assert_eq!(status, axum::http::StatusCode::OK);
        let resp: SearchResponse = serde_json::from_value(body).unwrap();
        assert_eq!(resp.results.len(), 1);
        assert_eq!(resp.results[0].record_type, RecordType::Checkpoint);
    }

    #[tokio::test]
    async fn search_with_limit_caps_results() {
        let state = setup_state();
        {
            let conn = state.db.lock().unwrap();
            for i in 0..5 {
                MeshStorage::store_remote_record(
                    &conn,
                    &format!("lesson-{i}"),
                    "lesson",
                    "node-pub",
                    &make_signed_lesson_json(&format!("lesson-{i}"), "Rust lesson"),
                    "public",
                )
                .unwrap();
            }
        }

        let app = search_router(state);
        let envelope = make_search_envelope("Rust", None, Some(3));
        let (status, body) = do_search(&app, &envelope).await;

        assert_eq!(status, axum::http::StatusCode::OK);
        let resp: SearchResponse = serde_json::from_value(body).unwrap();
        assert_eq!(resp.results.len(), 3);
        assert!(resp.truncated);
    }

    #[tokio::test]
    async fn search_no_matches_returns_empty() {
        let state = setup_state();
        {
            let conn = state.db.lock().unwrap();
            MeshStorage::store_remote_record(
                &conn,
                "lesson-1",
                "lesson",
                "node-pub",
                &make_signed_lesson_json("lesson-1", "Intro to Rust"),
                "public",
            )
            .unwrap();
        }

        let app = search_router(state);
        let envelope = make_search_envelope("nonexistent-xyz-query", None, None);
        let (status, body) = do_search(&app, &envelope).await;

        assert_eq!(status, axum::http::StatusCode::OK);
        let resp: SearchResponse = serde_json::from_value(body).unwrap();
        assert!(resp.results.is_empty());
        assert!(!resp.truncated);
    }

    #[test]
    fn calculate_text_score_basic() {
        let score = calculate_text_score("Rust ownership", r#"{"title":"Intro to Rust"}"#);
        assert!((score - 0.5).abs() < f64::EPSILON); // 1 of 2 words match

        let score = calculate_text_score("Rust", r#"{"title":"Intro to Rust"}"#);
        assert!((score - 1.0).abs() < f64::EPSILON); // 1 of 1 words match

        let score = calculate_text_score("Python", r#"{"title":"Intro to Rust"}"#);
        assert!((score - 0.0).abs() < f64::EPSILON); // 0 of 1 words match
    }

    #[test]
    fn calculate_text_score_empty_query() {
        let score = calculate_text_score("", r#"{"title":"anything"}"#);
        assert!((score - 0.0).abs() < f64::EPSILON);
    }
}
