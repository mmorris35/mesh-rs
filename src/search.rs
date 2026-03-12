use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::Utc;
use rusqlite::Connection;
use tracing::{info, warn};
use uuid::Uuid;

use crate::envelope::MeshEnvelope;
use crate::error::{MeshError, MeshResult};
use crate::identity::NodeIdentity;
use crate::storage::MeshStorage;
use crate::types::*;

/// Federated search across the MESH network.
///
/// Queries local records and all trusted peers concurrently, then merges,
/// deduplicates, and ranks the combined results.
pub struct FederatedSearch {
    identity: Arc<NodeIdentity>,
    db: Arc<Mutex<Connection>>,
    http_client: reqwest::Client,
}

impl FederatedSearch {
    /// Create a new `FederatedSearch` with the given identity, database, and HTTP client.
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

    /// Send a search request to a single peer.
    ///
    /// Wraps the request in a `MeshEnvelope`, POSTs to the peer's search endpoint,
    /// and parses the response. Returns an empty `SearchResponse` on timeout or error.
    pub async fn search_peer(
        &self,
        peer: &PeerConnection,
        request: &SearchRequest,
    ) -> MeshResult<SearchResponse> {
        let envelope = MeshEnvelope::new(
            self.identity.node_id(),
            "search",
            serde_json::to_value(request)?,
        );

        let url = format!("{}/mesh/v1/search", peer.endpoint.trim_end_matches('/'));

        let response = match self
            .http_client
            .post(&url)
            .json(&envelope)
            .timeout(Duration::from_secs(10))
            .send()
            .await
        {
            Ok(resp) => resp,
            Err(e) => {
                warn!(
                    peer_id = %peer.node_id,
                    error = %e,
                    "search_peer: request failed"
                );
                return Ok(empty_search_response(&request.request_id));
            }
        };

        match response.json::<SearchResponse>().await {
            Ok(search_response) => Ok(search_response),
            Err(e) => {
                warn!(
                    peer_id = %peer.node_id,
                    error = %e,
                    "search_peer: failed to parse response"
                );
                Ok(empty_search_response(&request.request_id))
            }
        }
    }

    /// Search all trusted peers concurrently.
    ///
    /// Returns a vector of `(node_id, SearchResponse)` pairs. Failures are logged
    /// and result in empty responses (they do not prevent other peers from being queried).
    pub async fn search_all_peers(&self, request: &SearchRequest) -> Vec<(String, SearchResponse)> {
        let peers = {
            let conn = match self.db.lock() {
                Ok(c) => c,
                Err(e) => {
                    warn!("search_all_peers: failed to acquire db lock: {e}");
                    return Vec::new();
                }
            };
            match MeshStorage::get_trusted_peers(&conn) {
                Ok(p) => p,
                Err(e) => {
                    warn!("search_all_peers: failed to get trusted peers: {e}");
                    return Vec::new();
                }
            }
        };

        if peers.is_empty() {
            return Vec::new();
        }

        let mut handles = Vec::new();
        for peer in peers {
            let request = request.clone();
            let node_id = peer.node_id.clone();
            let endpoint = peer.endpoint.clone();
            let identity = self.identity.clone();
            let http_client = self.http_client.clone();

            let handle = tokio::spawn(async move {
                let peer_conn = PeerConnection {
                    node_id: node_id.clone(),
                    endpoint,
                    trust_level: peer.trust_level,
                    last_seen: peer.last_seen,
                    connected_since: peer.connected_since,
                };

                let envelope = MeshEnvelope::new(
                    identity.node_id(),
                    "search",
                    serde_json::to_value(&request).unwrap_or_default(),
                );

                let url = format!(
                    "{}/mesh/v1/search",
                    peer_conn.endpoint.trim_end_matches('/')
                );

                let response = match http_client
                    .post(&url)
                    .json(&envelope)
                    .timeout(Duration::from_secs(10))
                    .send()
                    .await
                {
                    Ok(resp) => resp,
                    Err(e) => {
                        warn!(
                            peer_id = %node_id,
                            error = %e,
                            "search_all_peers: request failed"
                        );
                        return (node_id, empty_search_response(&request.request_id));
                    }
                };

                match response.json::<SearchResponse>().await {
                    Ok(search_response) => (node_id, search_response),
                    Err(e) => {
                        warn!(
                            peer_id = %node_id,
                            error = %e,
                            "search_all_peers: failed to parse response"
                        );
                        (node_id, empty_search_response(&request.request_id))
                    }
                }
            });

            handles.push(handle);
        }

        let mut results = Vec::new();
        for handle in handles {
            match handle.await {
                Ok(pair) => results.push(pair),
                Err(e) => {
                    warn!("search_all_peers: task join error: {e}");
                }
            }
        }

        results
    }

    /// Main entry point for federated search.
    ///
    /// Queries local records and all trusted peers concurrently, merges and
    /// deduplicates results, ranks by `relevance * trust_score * freshness`,
    /// and applies a limit (default 20, max 100).
    pub async fn search(
        &self,
        query: &str,
        record_types: Option<Vec<RecordType>>,
        filters: Option<SearchFilters>,
        limit: Option<usize>,
    ) -> MeshResult<SearchResponse> {
        let request_id = Uuid::new_v4().to_string();
        let effective_limit = limit.unwrap_or(20).min(100);

        let request = SearchRequest {
            query: query.to_string(),
            record_types: record_types.clone(),
            filters: filters.clone(),
            limit: Some(effective_limit),
            request_id: request_id.clone(),
            origin: self.identity.node_id().to_string(),
        };

        // Query local records
        let local_results = self.search_local(query, record_types.as_deref())?;

        // Query all trusted peers concurrently
        let peer_results = self.search_all_peers(&request).await;

        info!(
            query = %query,
            local_count = local_results.len(),
            peer_count = peer_results.len(),
            "federated search complete"
        );

        // Merge all results
        let mut merged = merge_results(local_results, peer_results);

        // Apply limit
        merged.truncate(effective_limit);

        let truncated = merged.len() >= effective_limit;

        Ok(SearchResponse {
            request_id,
            results: merged,
            truncated,
        })
    }

    /// Search local records from storage.
    fn search_local(
        &self,
        query: &str,
        record_types: Option<&[RecordType]>,
    ) -> MeshResult<Vec<SearchResult>> {
        let record_type_filter = record_types.and_then(|types| {
            if types.len() == 1 {
                match types[0] {
                    RecordType::Lesson => Some("lesson"),
                    RecordType::Checkpoint => Some("checkpoint"),
                }
            } else {
                None
            }
        });

        let conn = self
            .db
            .lock()
            .map_err(|e| MeshError::StorageError(e.to_string()))?;

        let records = MeshStorage::search_remote_records(&conn, query, record_type_filter, 200)?;

        let results: Vec<SearchResult> = records
            .into_iter()
            .filter_map(|record| {
                let value: serde_json::Value = serde_json::from_str(&record.signed_record).ok()?;
                let score = calculate_text_score(query, &record.signed_record);

                match record.record_type.as_str() {
                    "lesson" => {
                        let signed_lesson: SignedLesson = serde_json::from_value(value).ok()?;
                        let freshness = calculate_freshness(signed_lesson.publication.published_at);
                        Some(SearchResult {
                            record_type: RecordType::Lesson,
                            signed_lesson: Some(signed_lesson),
                            signed_checkpoint: None,
                            score: score * freshness,
                            trust_score: 1.0,
                            via: None,
                        })
                    }
                    "checkpoint" => {
                        let signed_checkpoint: SignedCheckpoint =
                            serde_json::from_value(value).ok()?;
                        let freshness =
                            calculate_freshness(signed_checkpoint.publication.published_at);
                        Some(SearchResult {
                            record_type: RecordType::Checkpoint,
                            signed_lesson: None,
                            signed_checkpoint: Some(signed_checkpoint),
                            score: score * freshness,
                            trust_score: 1.0,
                            via: None,
                        })
                    }
                    _ => None,
                }
            })
            .collect();

        Ok(results)
    }
}

/// Calculate freshness multiplier based on publication timestamp.
///
/// - Last 24 hours: 1.0
/// - Last week: 0.8
/// - Last month: 0.6
/// - Older: 0.4
pub fn calculate_freshness(published_at: i64) -> f64 {
    let now = Utc::now().timestamp_millis();
    let age_ms = now - published_at;

    let one_day_ms = 24 * 60 * 60 * 1000;
    let one_week_ms = 7 * one_day_ms;
    let one_month_ms = 30 * one_day_ms;

    if age_ms < one_day_ms {
        1.0
    } else if age_ms < one_week_ms {
        0.8
    } else if age_ms < one_month_ms {
        0.6
    } else {
        0.4
    }
}

/// Simple text relevance scoring (word match ratio).
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

/// Extract a unique record ID from a search result.
///
/// Looks for `signed_lesson.lesson["id"]` or `signed_checkpoint.checkpoint["id"]`.
pub fn get_record_id(result: &SearchResult) -> Option<String> {
    if let Some(ref lesson) = result.signed_lesson {
        lesson
            .lesson
            .get("id")
            .and_then(|v| v.as_str())
            .map(String::from)
    } else if let Some(ref checkpoint) = result.signed_checkpoint {
        checkpoint
            .checkpoint
            .get("id")
            .and_then(|v| v.as_str())
            .map(String::from)
    } else {
        None
    }
}

/// Merge local results with peer results, deduplicate, and sort by final score.
///
/// Peer results have their `via` field set to the originating node ID.
/// When the same record appears from multiple sources, the highest-scoring copy is kept.
/// Results are sorted by `score * trust_score` descending.
pub fn merge_results(
    local: Vec<SearchResult>,
    peer_results: Vec<(String, SearchResponse)>,
) -> Vec<SearchResult> {
    // Use a map keyed by record_id for deduplication.
    // For results without an ID, we keep them all (using a unique key).
    let mut best: HashMap<String, SearchResult> = HashMap::new();
    let mut no_id_results: Vec<SearchResult> = Vec::new();

    // Insert local results
    for result in local {
        match get_record_id(&result) {
            Some(id) => {
                let existing = best.get(&id);
                if existing.is_none()
                    || result.score * result.trust_score
                        > existing.unwrap().score * existing.unwrap().trust_score
                {
                    best.insert(id, result);
                }
            }
            None => {
                no_id_results.push(result);
            }
        }
    }

    // Insert peer results
    for (node_id, response) in peer_results {
        for mut result in response.results {
            result.via = Some(node_id.clone());
            match get_record_id(&result) {
                Some(id) => {
                    let existing = best.get(&id);
                    if existing.is_none()
                        || result.score * result.trust_score
                            > existing.unwrap().score * existing.unwrap().trust_score
                    {
                        best.insert(id, result);
                    }
                }
                None => {
                    no_id_results.push(result);
                }
            }
        }
    }

    let mut all_results: Vec<SearchResult> = best.into_values().chain(no_id_results).collect();

    // Sort by final score descending
    all_results.sort_by(|a, b| {
        let score_a = a.score * a.trust_score;
        let score_b = b.score * b.trust_score;
        score_b
            .partial_cmp(&score_a)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    all_results
}

/// Create an empty search response with the given request ID.
fn empty_search_response(request_id: &str) -> SearchResponse {
    SearchResponse {
        request_id: request_id.to_string(),
        results: Vec::new(),
        truncated: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::run_migrations;

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
            published_at: Utc::now().timestamp_millis(), // fresh
            topics: Some(vec!["math".to_string()]),
        }
    }

    fn make_lesson_result(id: &str, title: &str, score: f64, via: Option<&str>) -> SearchResult {
        SearchResult {
            record_type: RecordType::Lesson,
            signed_lesson: Some(SignedLesson {
                lesson: serde_json::json!({"id": id, "title": title}),
                publication: sample_publication(),
                signature: sample_signature_block(),
            }),
            signed_checkpoint: None,
            score,
            trust_score: 1.0,
            via: via.map(String::from),
        }
    }

    fn make_checkpoint_result(
        id: &str,
        title: &str,
        score: f64,
        via: Option<&str>,
    ) -> SearchResult {
        SearchResult {
            record_type: RecordType::Checkpoint,
            signed_lesson: None,
            signed_checkpoint: Some(SignedCheckpoint {
                checkpoint: serde_json::json!({"id": id, "title": title}),
                publication: sample_publication(),
                signature: sample_signature_block(),
            }),
            score,
            trust_score: 1.0,
            via: via.map(String::from),
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

    // -----------------------------------------------------------------------
    // merge_results tests
    // -----------------------------------------------------------------------

    #[test]
    fn merge_local_and_peer_results_combined_correctly() {
        let local = vec![make_lesson_result("lesson-1", "Intro to Rust", 0.9, None)];
        let peer_results = vec![(
            "peer-1".to_string(),
            SearchResponse {
                request_id: "req-1".to_string(),
                results: vec![make_lesson_result("lesson-2", "Advanced Rust", 0.8, None)],
                truncated: false,
            },
        )];

        let merged = merge_results(local, peer_results);
        assert_eq!(merged.len(), 2);
        // Both results should be present
        let ids: Vec<String> = merged.iter().filter_map(get_record_id).collect();
        assert!(ids.contains(&"lesson-1".to_string()));
        assert!(ids.contains(&"lesson-2".to_string()));
        // Peer result should have via set
        let peer_result = merged
            .iter()
            .find(|r| get_record_id(r) == Some("lesson-2".to_string()))
            .unwrap();
        assert_eq!(peer_result.via, Some("peer-1".to_string()));
    }

    #[test]
    fn deduplication_keeps_highest_score() {
        let local = vec![make_lesson_result("lesson-1", "Intro to Rust", 0.5, None)];
        let peer_results = vec![(
            "peer-1".to_string(),
            SearchResponse {
                request_id: "req-1".to_string(),
                results: vec![make_lesson_result("lesson-1", "Intro to Rust", 0.9, None)],
                truncated: false,
            },
        )];

        let merged = merge_results(local, peer_results);
        assert_eq!(merged.len(), 1);
        assert!((merged[0].score - 0.9).abs() < f64::EPSILON);
    }

    #[test]
    fn ranking_order_correct() {
        let local = vec![
            make_lesson_result("lesson-1", "Low Score", 0.3, None),
            make_lesson_result("lesson-2", "High Score", 0.9, None),
            make_lesson_result("lesson-3", "Mid Score", 0.6, None),
        ];

        let merged = merge_results(local, vec![]);
        assert_eq!(merged.len(), 3);
        assert!((merged[0].score - 0.9).abs() < f64::EPSILON);
        assert!((merged[1].score - 0.6).abs() < f64::EPSILON);
        assert!((merged[2].score - 0.3).abs() < f64::EPSILON);
    }

    #[test]
    fn freshness_calculation_last_24h() {
        let now = Utc::now().timestamp_millis();
        let freshness = calculate_freshness(now - 1000); // 1 second ago
        assert!((freshness - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn freshness_calculation_last_week() {
        let now = Utc::now().timestamp_millis();
        let two_days_ago = now - 2 * 24 * 60 * 60 * 1000;
        let freshness = calculate_freshness(two_days_ago);
        assert!((freshness - 0.8).abs() < f64::EPSILON);
    }

    #[test]
    fn freshness_calculation_last_month() {
        let now = Utc::now().timestamp_millis();
        let two_weeks_ago = now - 14 * 24 * 60 * 60 * 1000;
        let freshness = calculate_freshness(two_weeks_ago);
        assert!((freshness - 0.6).abs() < f64::EPSILON);
    }

    #[test]
    fn freshness_calculation_older() {
        let freshness = calculate_freshness(1600000000000); // old timestamp
        assert!((freshness - 0.4).abs() < f64::EPSILON);
    }

    #[test]
    fn limit_applied_after_merge() {
        let local: Vec<SearchResult> = (0..10)
            .map(|i| {
                make_lesson_result(&format!("lesson-{i}"), "Rust", 0.5 + i as f64 * 0.01, None)
            })
            .collect();
        let peer_results = vec![(
            "peer-1".to_string(),
            SearchResponse {
                request_id: "req-1".to_string(),
                results: (10..20)
                    .map(|i| {
                        make_lesson_result(
                            &format!("lesson-{i}"),
                            "Rust",
                            0.5 + i as f64 * 0.01,
                            None,
                        )
                    })
                    .collect(),
                truncated: false,
            },
        )];

        let mut merged = merge_results(local, peer_results);
        assert_eq!(merged.len(), 20);
        // Apply limit like the search method does
        merged.truncate(5);
        assert_eq!(merged.len(), 5);
    }

    #[test]
    fn empty_peer_results_dont_affect_local() {
        let local = vec![
            make_lesson_result("lesson-1", "Rust", 0.9, None),
            make_checkpoint_result("cp-1", "Quiz", 0.8, None),
        ];

        let merged = merge_results(local, vec![]);
        assert_eq!(merged.len(), 2);
        let ids: Vec<String> = merged.iter().filter_map(get_record_id).collect();
        assert!(ids.contains(&"lesson-1".to_string()));
        assert!(ids.contains(&"cp-1".to_string()));
    }

    #[test]
    fn get_record_id_extracts_lesson_id() {
        let result = make_lesson_result("lesson-42", "Test", 1.0, None);
        assert_eq!(get_record_id(&result), Some("lesson-42".to_string()));
    }

    #[test]
    fn get_record_id_extracts_checkpoint_id() {
        let result = make_checkpoint_result("cp-7", "Test", 1.0, None);
        assert_eq!(get_record_id(&result), Some("cp-7".to_string()));
    }

    #[test]
    fn get_record_id_returns_none_for_empty_result() {
        let result = SearchResult {
            record_type: RecordType::Lesson,
            signed_lesson: None,
            signed_checkpoint: None,
            score: 0.0,
            trust_score: 0.0,
            via: None,
        };
        assert_eq!(get_record_id(&result), None);
    }

    #[tokio::test]
    async fn search_peer_unreachable_returns_empty() {
        let identity = Arc::new(NodeIdentity::generate());
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();
        let db = Arc::new(Mutex::new(conn));
        let client = reqwest::Client::new();

        let fs = FederatedSearch::new(identity, db, client);

        let peer = PeerConnection {
            node_id: "unreachable-node".to_string(),
            endpoint: "http://127.0.0.1:1".to_string(),
            trust_level: TrustLevel::Full,
            last_seen: None,
            connected_since: None,
        };

        let request = SearchRequest {
            query: "test".to_string(),
            record_types: None,
            filters: None,
            limit: Some(10),
            request_id: "req-test".to_string(),
            origin: "self".to_string(),
        };

        let response = fs.search_peer(&peer, &request).await.unwrap();
        assert!(response.results.is_empty());
        assert!(!response.truncated);
    }

    #[tokio::test]
    async fn search_local_returns_results() {
        let identity = Arc::new(NodeIdentity::generate());
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();

        // Insert test records
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

        let db = Arc::new(Mutex::new(conn));
        let client = reqwest::Client::new();
        let fs = FederatedSearch::new(identity, db, client);

        let results = fs.search_local("Rust", None).unwrap();
        assert_eq!(results.len(), 2);
    }

    #[tokio::test]
    async fn search_returns_merged_local_results() {
        let identity = Arc::new(NodeIdentity::generate());
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();

        MeshStorage::store_remote_record(
            &conn,
            "lesson-1",
            "lesson",
            "node-pub",
            &make_signed_lesson_json("lesson-1", "Intro to Rust"),
            "public",
        )
        .unwrap();

        let db = Arc::new(Mutex::new(conn));
        let client = reqwest::Client::new();
        let fs = FederatedSearch::new(identity, db, client);

        let response = fs.search("Rust", None, None, None).await.unwrap();
        assert!(!response.request_id.is_empty());
        assert_eq!(response.results.len(), 1);
    }

    #[tokio::test]
    async fn search_applies_limit() {
        let identity = Arc::new(NodeIdentity::generate());
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();

        for i in 0..10 {
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

        let db = Arc::new(Mutex::new(conn));
        let client = reqwest::Client::new();
        let fs = FederatedSearch::new(identity, db, client);

        let response = fs.search("Rust", None, None, Some(3)).await.unwrap();
        assert_eq!(response.results.len(), 3);
        assert!(response.truncated);
    }

    #[test]
    fn deduplication_across_multiple_peers() {
        let local = vec![make_lesson_result("lesson-1", "Rust", 0.5, None)];
        let peer_results = vec![
            (
                "peer-1".to_string(),
                SearchResponse {
                    request_id: "req-1".to_string(),
                    results: vec![make_lesson_result("lesson-1", "Rust", 0.7, None)],
                    truncated: false,
                },
            ),
            (
                "peer-2".to_string(),
                SearchResponse {
                    request_id: "req-1".to_string(),
                    results: vec![make_lesson_result("lesson-1", "Rust", 0.9, None)],
                    truncated: false,
                },
            ),
        ];

        let merged = merge_results(local, peer_results);
        assert_eq!(merged.len(), 1);
        assert!((merged[0].score - 0.9).abs() < f64::EPSILON);
    }

    #[test]
    fn merge_mixed_record_types() {
        let local = vec![
            make_lesson_result("lesson-1", "Rust Basics", 0.9, None),
            make_checkpoint_result("cp-1", "Rust Quiz", 0.8, None),
        ];
        let peer_results = vec![(
            "peer-1".to_string(),
            SearchResponse {
                request_id: "req-1".to_string(),
                results: vec![
                    make_lesson_result("lesson-2", "Advanced Rust", 0.7, None),
                    make_checkpoint_result("cp-2", "Rust Exam", 0.6, None),
                ],
                truncated: false,
            },
        )];

        let merged = merge_results(local, peer_results);
        assert_eq!(merged.len(), 4);
        // Should be sorted by score descending
        assert!((merged[0].score - 0.9).abs() < f64::EPSILON);
        assert!((merged[1].score - 0.8).abs() < f64::EPSILON);
        assert!((merged[2].score - 0.7).abs() < f64::EPSILON);
        assert!((merged[3].score - 0.6).abs() < f64::EPSILON);
    }
}
