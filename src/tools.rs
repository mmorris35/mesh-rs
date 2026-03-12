use std::sync::{Arc, Mutex};

use rusqlite::Connection;
use serde_json::json;

use crate::error::{MeshError, MeshResult};
use crate::identity::NodeIdentity;
use crate::peer::PeerManager;
use crate::publish::Publisher;
use crate::revoke::Revoker;
use crate::search::FederatedSearch;
use crate::trust::TrustManager;
use crate::types::{RecordType, Visibility};

/// Trait for fetching and managing records by ID.
pub trait RecordFetcher: Send + Sync {
    /// Fetch a lesson by ID, returning its JSON representation if found.
    fn fetch_lesson(&self, id: &str) -> MeshResult<Option<serde_json::Value>>;
    /// Fetch a checkpoint by ID, returning its JSON representation if found.
    fn fetch_checkpoint(&self, id: &str) -> MeshResult<Option<serde_json::Value>>;
    /// Set the visibility for a record.
    fn set_visibility(
        &self,
        id: &str,
        record_type: RecordType,
        visibility: Visibility,
    ) -> MeshResult<()>;
}

/// High-level MCP tool handlers that orchestrate MESH operations.
pub struct MeshTools {
    identity: Arc<NodeIdentity>,
    db: Arc<Mutex<Connection>>,
    peer_manager: Arc<PeerManager>,
    trust_manager: Arc<TrustManager>,
    publisher: Arc<Publisher>,
    revoker: Arc<Revoker>,
    search: Arc<FederatedSearch>,
}

impl MeshTools {
    /// Create a new `MeshTools` instance with all required dependencies.
    pub fn new(
        identity: Arc<NodeIdentity>,
        db: Arc<Mutex<Connection>>,
        peer_manager: Arc<PeerManager>,
        trust_manager: Arc<TrustManager>,
        publisher: Arc<Publisher>,
        revoker: Arc<Revoker>,
        search: Arc<FederatedSearch>,
    ) -> Self {
        Self {
            identity,
            db,
            peer_manager,
            trust_manager,
            publisher,
            revoker,
            search,
        }
    }

    /// Return status information about this MESH node.
    ///
    /// Returns JSON with node_id, fingerprint, peer_count, trusted_peer_count,
    /// and cached_remote_records count.
    pub fn mesh_status(&self) -> MeshResult<serde_json::Value> {
        let conn = self
            .db
            .lock()
            .map_err(|e| MeshError::StorageError(format!("failed to acquire db lock: {e}")))?;

        let peer_count: i64 =
            conn.query_row("SELECT COUNT(*) FROM mesh_peers", [], |row| row.get(0))?;

        let trusted_peer_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM mesh_peers WHERE trust_level = 'full'",
            [],
            |row| row.get(0),
        )?;

        let cached_remote_records: i64 =
            conn.query_row("SELECT COUNT(*) FROM mesh_remote_records", [], |row| {
                row.get(0)
            })?;

        Ok(json!({
            "node_id": self.identity.node_id(),
            "fingerprint": self.identity.fingerprint(),
            "peer_count": peer_count,
            "trusted_peer_count": trusted_peer_count,
            "cached_remote_records": cached_remote_records,
        }))
    }

    /// Manage peers: list, add, or remove.
    ///
    /// - `"list"`: returns an array of all peers.
    /// - `"add"`: adds a peer (requires `node_id` and `endpoint`).
    /// - `"remove"`: removes a peer (requires `node_id`).
    pub fn mesh_peers(
        &self,
        action: &str,
        node_id: Option<&str>,
        endpoint: Option<&str>,
    ) -> MeshResult<serde_json::Value> {
        match action {
            "list" => {
                let peers = self.peer_manager.list_peers()?;
                let peer_values: Vec<serde_json::Value> = peers
                    .iter()
                    .map(|p| {
                        json!({
                            "node_id": p.node_id,
                            "endpoint": p.endpoint,
                            "trust_level": format!("{:?}", p.trust_level).to_lowercase(),
                            "last_seen": p.last_seen,
                            "connected_since": p.connected_since,
                        })
                    })
                    .collect();
                Ok(json!(peer_values))
            }
            "add" => {
                let nid = node_id.ok_or_else(|| {
                    MeshError::InvalidRequest("node_id is required for add".to_string())
                })?;
                let ep = endpoint.ok_or_else(|| {
                    MeshError::InvalidRequest("endpoint is required for add".to_string())
                })?;
                self.peer_manager.add_peer(nid, ep)?;
                Ok(json!({
                    "added": true,
                    "node_id": nid,
                }))
            }
            "remove" => {
                let nid = node_id.ok_or_else(|| {
                    MeshError::InvalidRequest("node_id is required for remove".to_string())
                })?;
                self.peer_manager.remove_peer(nid)?;
                Ok(json!({
                    "removed": true,
                    "node_id": nid,
                }))
            }
            _ => Err(MeshError::InvalidRequest(format!(
                "unknown peers action: {action}"
            ))),
        }
    }

    /// Manage trust: add, remove, or list trusted peers.
    ///
    /// - `"add"`: grant trust to a peer (requires `node_id`).
    /// - `"remove"`: revoke trust from a peer (requires `node_id`).
    /// - `"list"`: return all trusted peers.
    pub fn mesh_trust(&self, action: &str, node_id: Option<&str>) -> MeshResult<serde_json::Value> {
        match action {
            "add" => {
                let nid = node_id.ok_or_else(|| {
                    MeshError::InvalidRequest("node_id is required for add".to_string())
                })?;
                self.trust_manager.add_trust(nid)?;
                Ok(json!({
                    "trusted": true,
                    "node_id": nid,
                }))
            }
            "remove" => {
                let nid = node_id.ok_or_else(|| {
                    MeshError::InvalidRequest("node_id is required for remove".to_string())
                })?;
                self.trust_manager.remove_trust(nid)?;
                Ok(json!({
                    "trusted": false,
                    "node_id": nid,
                }))
            }
            "list" => {
                let trusted = self.trust_manager.list_trusted()?;
                let values: Vec<serde_json::Value> = trusted
                    .iter()
                    .map(|p| {
                        json!({
                            "node_id": p.node_id,
                            "endpoint": p.endpoint,
                            "trust_level": "full",
                        })
                    })
                    .collect();
                Ok(json!(values))
            }
            _ => Err(MeshError::InvalidRequest(format!(
                "unknown trust action: {action}"
            ))),
        }
    }

    /// Publish a record (lesson or checkpoint) and announce it to trusted peers.
    ///
    /// - `visibility`: defaults to `"public"`, rejects `"private"`.
    /// - `record_type`: must be `"lesson"` or `"checkpoint"`.
    pub async fn mesh_publish(
        &self,
        record: serde_json::Value,
        record_type: &str,
        visibility: Option<&str>,
        topics: Option<Vec<String>>,
    ) -> MeshResult<serde_json::Value> {
        let vis_str = visibility.unwrap_or("public");
        let vis = match vis_str {
            "public" => Visibility::Public,
            "unlisted" => Visibility::Unlisted,
            "private" => {
                return Err(MeshError::InvalidRequest(
                    "private visibility is not allowed for mesh publishing".to_string(),
                ))
            }
            other => {
                return Err(MeshError::InvalidRequest(format!(
                    "unknown visibility: {other}"
                )))
            }
        };

        match record_type {
            "lesson" => {
                let (signed, results) = self
                    .publisher
                    .publish_and_announce_lesson(record, vis, topics)
                    .await?;
                let announced_to: Vec<String> = results
                    .iter()
                    .filter(|(_, r)| r.is_ok())
                    .map(|(node_id, _)| node_id.clone())
                    .collect();
                Ok(json!({
                    "published": true,
                    "record_type": "lesson",
                    "visibility": vis_str,
                    "signature": {
                        "node_id": signed.signature.node_id,
                        "timestamp": signed.signature.timestamp,
                    },
                    "announced_to": announced_to,
                }))
            }
            "checkpoint" => {
                let (signed, results) = self
                    .publisher
                    .publish_and_announce_checkpoint(record, vis, topics)
                    .await?;
                let announced_to: Vec<String> = results
                    .iter()
                    .filter(|(_, r)| r.is_ok())
                    .map(|(node_id, _)| node_id.clone())
                    .collect();
                Ok(json!({
                    "published": true,
                    "record_type": "checkpoint",
                    "visibility": vis_str,
                    "signature": {
                        "node_id": signed.signature.node_id,
                        "timestamp": signed.signature.timestamp,
                    },
                    "announced_to": announced_to,
                }))
            }
            other => Err(MeshError::InvalidRequest(format!(
                "unknown record_type: {other}"
            ))),
        }
    }

    /// Search the federated mesh network.
    ///
    /// Parses `record_types` strings into `RecordType` enums, invokes federated search,
    /// and transforms results into an agent-friendly format.
    pub async fn mesh_search(
        &self,
        query: &str,
        record_types: Option<Vec<String>>,
        limit: Option<usize>,
    ) -> MeshResult<serde_json::Value> {
        let parsed_types = match record_types {
            Some(ref types) => {
                let mut parsed = Vec::new();
                for t in types {
                    match t.as_str() {
                        "lesson" => parsed.push(RecordType::Lesson),
                        "checkpoint" => parsed.push(RecordType::Checkpoint),
                        other => {
                            return Err(MeshError::InvalidRequest(format!(
                                "unknown record_type: {other}"
                            )))
                        }
                    }
                }
                Some(parsed)
            }
            None => None,
        };

        let response = self.search.search(query, parsed_types, None, limit).await?;

        let results: Vec<serde_json::Value> = response
            .results
            .iter()
            .map(|r| {
                let record_data = if let Some(ref sl) = r.signed_lesson {
                    json!({
                        "record_type": "lesson",
                        "record": sl.lesson,
                        "publication": {
                            "visibility": sl.publication.visibility,
                            "published_at": sl.publication.published_at,
                            "topics": sl.publication.topics,
                        },
                        "signature": {
                            "node_id": sl.signature.node_id,
                            "timestamp": sl.signature.timestamp,
                        },
                        "score": r.score,
                        "trust_score": r.trust_score,
                        "via": r.via,
                    })
                } else if let Some(ref sc) = r.signed_checkpoint {
                    json!({
                        "record_type": "checkpoint",
                        "record": sc.checkpoint,
                        "publication": {
                            "visibility": sc.publication.visibility,
                            "published_at": sc.publication.published_at,
                            "topics": sc.publication.topics,
                        },
                        "signature": {
                            "node_id": sc.signature.node_id,
                            "timestamp": sc.signature.timestamp,
                        },
                        "score": r.score,
                        "trust_score": r.trust_score,
                        "via": r.via,
                    })
                } else {
                    json!({
                        "record_type": format!("{:?}", r.record_type).to_lowercase(),
                        "score": r.score,
                        "trust_score": r.trust_score,
                        "via": r.via,
                    })
                };
                record_data
            })
            .collect();

        // Count peers queried (trusted peers)
        let peers_queried = {
            let conn = self
                .db
                .lock()
                .map_err(|e| MeshError::StorageError(format!("failed to acquire db lock: {e}")))?;
            let count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM mesh_peers WHERE trust_level = 'full'",
                [],
                |row| row.get(0),
            )?;
            count as usize
        };

        let total_results = results.len();

        Ok(json!({
            "results": results,
            "total_results": total_results,
            "truncated": response.truncated,
            "peers_queried": peers_queried,
            "peers_responded": peers_queried,
        }))
    }

    /// Revoke a record and announce the revocation to trusted peers.
    pub async fn mesh_revoke(
        &self,
        record_id: &str,
        record_type: &str,
        reason: Option<&str>,
    ) -> MeshResult<serde_json::Value> {
        let rt = match record_type {
            "lesson" => RecordType::Lesson,
            "checkpoint" => RecordType::Checkpoint,
            other => {
                return Err(MeshError::InvalidRequest(format!(
                    "unknown record_type: {other}"
                )))
            }
        };

        let (revocation, results) = self
            .revoker
            .revoke_and_announce(record_id, rt, reason)
            .await?;

        let announced_to: Vec<String> = results
            .iter()
            .filter(|(_, r)| r.is_ok())
            .map(|(endpoint, _)| endpoint.clone())
            .collect();

        Ok(json!({
            "revoked": true,
            "record_id": revocation.record_id,
            "record_type": record_type,
            "reason": revocation.reason,
            "announced_to": announced_to,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{run_migrations, MeshStorage};
    use serde_json::json;

    fn setup() -> (
        Arc<NodeIdentity>,
        Arc<Mutex<Connection>>,
        Arc<PeerManager>,
        Arc<TrustManager>,
        Arc<Publisher>,
        Arc<Revoker>,
        Arc<FederatedSearch>,
    ) {
        let identity = Arc::new(NodeIdentity::generate());
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();
        let db = Arc::new(Mutex::new(conn));
        let client = reqwest::Client::new();

        let peer_manager = Arc::new(PeerManager::new(db.clone(), client.clone()));
        let trust_manager = Arc::new(TrustManager::new(db.clone()));
        let publisher = Arc::new(Publisher::new(identity.clone(), db.clone(), client.clone()));
        let revoker = Arc::new(Revoker::new(identity.clone(), db.clone(), client.clone()));
        let search = Arc::new(FederatedSearch::new(identity.clone(), db.clone(), client));

        (
            identity,
            db,
            peer_manager,
            trust_manager,
            publisher,
            revoker,
            search,
        )
    }

    fn make_tools() -> MeshTools {
        let (identity, db, peer_manager, trust_manager, publisher, revoker, search) = setup();
        MeshTools::new(
            identity,
            db,
            peer_manager,
            trust_manager,
            publisher,
            revoker,
            search,
        )
    }

    // -----------------------------------------------------------------------
    // mesh_status tests
    // -----------------------------------------------------------------------

    #[test]
    fn tools_mesh_status_returns_correct_node_info() {
        let tools = make_tools();
        let status = tools.mesh_status().unwrap();

        assert_eq!(status["node_id"], tools.identity.node_id());
        assert_eq!(status["fingerprint"], tools.identity.fingerprint());
        assert_eq!(status["peer_count"], 0);
        assert_eq!(status["trusted_peer_count"], 0);
        assert_eq!(status["cached_remote_records"], 0);
    }

    #[test]
    fn tools_mesh_status_reflects_peer_counts() {
        let tools = make_tools();

        // Add peers
        tools
            .peer_manager
            .add_peer("node-1", "https://node1.example.com")
            .unwrap();
        tools
            .peer_manager
            .add_peer("node-2", "https://node2.example.com")
            .unwrap();
        tools.trust_manager.add_trust("node-1").unwrap();

        let status = tools.mesh_status().unwrap();
        assert_eq!(status["peer_count"], 2);
        assert_eq!(status["trusted_peer_count"], 1);
    }

    // -----------------------------------------------------------------------
    // mesh_peers tests
    // -----------------------------------------------------------------------

    #[test]
    fn tools_mesh_peers_list_empty() {
        let tools = make_tools();
        let result = tools.mesh_peers("list", None, None).unwrap();
        assert_eq!(result.as_array().unwrap().len(), 0);
    }

    #[test]
    fn tools_mesh_peers_add_and_list() {
        let tools = make_tools();

        let add_result = tools
            .mesh_peers("add", Some("node-1"), Some("https://node1.example.com"))
            .unwrap();
        assert_eq!(add_result["added"], true);
        assert_eq!(add_result["node_id"], "node-1");

        let list_result = tools.mesh_peers("list", None, None).unwrap();
        let peers = list_result.as_array().unwrap();
        assert_eq!(peers.len(), 1);
        assert_eq!(peers[0]["node_id"], "node-1");
    }

    #[test]
    fn tools_mesh_peers_remove() {
        let tools = make_tools();

        tools
            .mesh_peers("add", Some("node-1"), Some("https://node1.example.com"))
            .unwrap();

        let remove_result = tools.mesh_peers("remove", Some("node-1"), None).unwrap();
        assert_eq!(remove_result["removed"], true);
        assert_eq!(remove_result["node_id"], "node-1");

        let list_result = tools.mesh_peers("list", None, None).unwrap();
        assert_eq!(list_result.as_array().unwrap().len(), 0);
    }

    #[test]
    fn tools_mesh_peers_add_missing_params() {
        let tools = make_tools();

        let result = tools.mesh_peers("add", None, Some("https://example.com"));
        assert!(result.is_err());

        let result = tools.mesh_peers("add", Some("node-1"), None);
        assert!(result.is_err());
    }

    #[test]
    fn tools_mesh_peers_unknown_action() {
        let tools = make_tools();
        let result = tools.mesh_peers("unknown", None, None);
        assert!(result.is_err());
    }

    // -----------------------------------------------------------------------
    // mesh_trust tests
    // -----------------------------------------------------------------------

    #[test]
    fn tools_mesh_trust_add_and_list() {
        let tools = make_tools();

        // Need a peer first
        tools
            .peer_manager
            .add_peer("node-1", "https://node1.example.com")
            .unwrap();

        let add_result = tools.mesh_trust("add", Some("node-1")).unwrap();
        assert_eq!(add_result["trusted"], true);
        assert_eq!(add_result["node_id"], "node-1");

        let list_result = tools.mesh_trust("list", None).unwrap();
        let trusted = list_result.as_array().unwrap();
        assert_eq!(trusted.len(), 1);
        assert_eq!(trusted[0]["node_id"], "node-1");
    }

    #[test]
    fn tools_mesh_trust_remove() {
        let tools = make_tools();

        tools
            .peer_manager
            .add_peer("node-1", "https://node1.example.com")
            .unwrap();
        tools.trust_manager.add_trust("node-1").unwrap();

        let remove_result = tools.mesh_trust("remove", Some("node-1")).unwrap();
        assert_eq!(remove_result["trusted"], false);
        assert_eq!(remove_result["node_id"], "node-1");

        let list_result = tools.mesh_trust("list", None).unwrap();
        assert_eq!(list_result.as_array().unwrap().len(), 0);
    }

    #[test]
    fn tools_mesh_trust_add_missing_node_id() {
        let tools = make_tools();
        let result = tools.mesh_trust("add", None);
        assert!(result.is_err());
    }

    #[test]
    fn tools_mesh_trust_unknown_action() {
        let tools = make_tools();
        let result = tools.mesh_trust("unknown", None);
        assert!(result.is_err());
    }

    // -----------------------------------------------------------------------
    // mesh_publish tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn tools_mesh_publish_creates_valid_signed_lesson() {
        let tools = make_tools();
        let lesson = json!({"id": "lesson-1", "title": "Learn Rust"});

        let result = tools
            .mesh_publish(lesson, "lesson", Some("public"), Some(vec!["rust".into()]))
            .await
            .unwrap();

        assert_eq!(result["published"], true);
        assert_eq!(result["record_type"], "lesson");
        assert_eq!(result["visibility"], "public");
        assert!(result["signature"]["node_id"].is_string());
        assert!(result["signature"]["timestamp"].is_number());
        assert!(result["announced_to"].is_array());
    }

    #[tokio::test]
    async fn tools_mesh_publish_creates_valid_signed_checkpoint() {
        let tools = make_tools();
        let checkpoint = json!({"id": "cp-1", "score": 95});

        let result = tools
            .mesh_publish(checkpoint, "checkpoint", None, None)
            .await
            .unwrap();

        assert_eq!(result["published"], true);
        assert_eq!(result["record_type"], "checkpoint");
        assert_eq!(result["visibility"], "public"); // default
    }

    #[tokio::test]
    async fn tools_mesh_publish_rejects_private() {
        let tools = make_tools();
        let lesson = json!({"id": "l-1"});

        let result = tools
            .mesh_publish(lesson, "lesson", Some("private"), None)
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn tools_mesh_publish_unknown_record_type() {
        let tools = make_tools();
        let result = tools.mesh_publish(json!({}), "unknown", None, None).await;
        assert!(result.is_err());
    }

    // -----------------------------------------------------------------------
    // mesh_revoke tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn tools_mesh_revoke_creates_valid_revocation() {
        let tools = make_tools();

        let result = tools
            .mesh_revoke("lesson-42", "lesson", Some("outdated"))
            .await
            .unwrap();

        assert_eq!(result["revoked"], true);
        assert_eq!(result["record_id"], "lesson-42");
        assert_eq!(result["record_type"], "lesson");
        assert_eq!(result["reason"], "outdated");
        assert!(result["announced_to"].is_array());
    }

    #[tokio::test]
    async fn tools_mesh_revoke_checkpoint() {
        let tools = make_tools();

        let result = tools.mesh_revoke("cp-5", "checkpoint", None).await.unwrap();

        assert_eq!(result["revoked"], true);
        assert_eq!(result["record_id"], "cp-5");
        assert_eq!(result["record_type"], "checkpoint");
        assert!(result["reason"].is_null());
    }

    #[tokio::test]
    async fn tools_mesh_revoke_unknown_type() {
        let tools = make_tools();
        let result = tools.mesh_revoke("x", "unknown", None).await;
        assert!(result.is_err());
    }

    // -----------------------------------------------------------------------
    // mesh_search tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn tools_mesh_search_returns_results_format() {
        let tools = make_tools();

        // Insert a record to find
        {
            let conn = tools.db.lock().unwrap();
            let signed_lesson = json!({
                "lesson": {"id": "l-1", "title": "Intro to Rust"},
                "publication": {
                    "visibility": "public",
                    "publishedAt": chrono::Utc::now().timestamp_millis(),
                    "topics": ["rust"]
                },
                "signature": {
                    "algorithm": "ed25519",
                    "nodeId": "test-node",
                    "publicKey": "dGVzdA==",
                    "timestamp": chrono::Utc::now().timestamp_millis(),
                    "sig": "dGVzdA=="
                }
            });
            MeshStorage::store_remote_record(
                &conn,
                "l-1",
                "lesson",
                "test-node",
                &serde_json::to_string(&signed_lesson).unwrap(),
                "public",
            )
            .unwrap();
        }

        let result = tools.mesh_search("Rust", None, Some(10)).await.unwrap();

        assert!(result["results"].is_array());
        assert!(result["total_results"].is_number());
        assert!(result.get("truncated").is_some());
        assert!(result["peers_queried"].is_number());
        assert!(result["peers_responded"].is_number());
        assert_eq!(result["total_results"], 1);
    }

    #[tokio::test]
    async fn tools_mesh_search_empty_results() {
        let tools = make_tools();

        let result = tools.mesh_search("nonexistent", None, None).await.unwrap();

        assert_eq!(result["total_results"], 0);
        assert_eq!(result["results"].as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn tools_mesh_search_unknown_record_type() {
        let tools = make_tools();
        let result = tools
            .mesh_search("test", Some(vec!["unknown".to_string()]), None)
            .await;
        assert!(result.is_err());
    }
}
