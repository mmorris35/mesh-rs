use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Visibility
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Visibility {
    #[default]
    Private,
    Unlisted,
    Public,
}

// ---------------------------------------------------------------------------
// Publication
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Publication {
    pub visibility: Visibility,
    pub published_at: i64,
    pub topics: Option<Vec<String>>,
}

// ---------------------------------------------------------------------------
// SignatureBlock
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignatureBlock {
    pub algorithm: String,
    pub node_id: String,
    pub public_key: String,
    pub timestamp: i64,
    pub sig: String,
}

// ---------------------------------------------------------------------------
// SignedLesson
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignedLesson {
    pub lesson: serde_json::Value,
    pub publication: Publication,
    pub signature: SignatureBlock,
}

// ---------------------------------------------------------------------------
// SignedCheckpoint
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignedCheckpoint {
    pub checkpoint: serde_json::Value,
    pub publication: Publication,
    pub signature: SignatureBlock,
}

// ---------------------------------------------------------------------------
// RecordType
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RecordType {
    Lesson,
    Checkpoint,
}

// ---------------------------------------------------------------------------
// TrustLevel
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TrustLevel {
    Full,
    None,
}

// ---------------------------------------------------------------------------
// PeerConnection
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PeerConnection {
    pub node_id: String,
    pub endpoint: String,
    pub trust_level: TrustLevel,
    pub last_seen: Option<i64>,
    pub connected_since: Option<i64>,
}

// ---------------------------------------------------------------------------
// Revocation (MESH spec §4.1)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Revocation {
    pub record_type: RecordType,
    pub record_id: String,
    pub node_id: String,
    pub revoked_at: i64,
    pub reason: Option<String>,
    pub signature: Option<SignatureBlock>,
}

// ---------------------------------------------------------------------------
// PublicationAnnouncement
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicationAnnouncement {
    pub record_type: RecordType,
    pub signed_lesson: Option<SignedLesson>,
    pub signed_checkpoint: Option<SignedCheckpoint>,
}

// ---------------------------------------------------------------------------
// RevocationAnnouncement
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RevocationAnnouncement {
    pub revocation: Revocation,
}

// ---------------------------------------------------------------------------
// SearchRequest
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchRequest {
    pub query: String,
    pub record_types: Option<Vec<RecordType>>,
    pub filters: Option<SearchFilters>,
    pub limit: Option<usize>,
    pub request_id: String,
    pub origin: String,
}

// ---------------------------------------------------------------------------
// SearchFilters
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchFilters {
    pub tags: Option<Vec<String>>,
    pub topics: Option<Vec<String>>,
    pub since: Option<i64>,
}

// ---------------------------------------------------------------------------
// SearchResponse
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResponse {
    pub request_id: String,
    pub results: Vec<SearchResult>,
    pub truncated: bool,
}

// ---------------------------------------------------------------------------
// SearchResult
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResult {
    pub record_type: RecordType,
    pub signed_lesson: Option<SignedLesson>,
    pub signed_checkpoint: Option<SignedCheckpoint>,
    pub score: f64,
    pub trust_score: f64,
    pub via: Option<String>,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json;

    #[test]
    fn visibility_default_is_private() {
        assert_eq!(Visibility::default(), Visibility::Private);
    }

    #[test]
    fn visibility_serde_roundtrip() {
        for (variant, expected_str) in [
            (Visibility::Private, "\"private\""),
            (Visibility::Unlisted, "\"unlisted\""),
            (Visibility::Public, "\"public\""),
        ] {
            let json = serde_json::to_string(&variant).unwrap();
            assert_eq!(json, expected_str);
            let back: Visibility = serde_json::from_str(&json).unwrap();
            assert_eq!(back, variant);
        }
    }

    #[test]
    fn record_type_serde_roundtrip() {
        for (variant, expected_str) in [
            (RecordType::Lesson, "\"lesson\""),
            (RecordType::Checkpoint, "\"checkpoint\""),
        ] {
            let json = serde_json::to_string(&variant).unwrap();
            assert_eq!(json, expected_str);
            let back: RecordType = serde_json::from_str(&json).unwrap();
            assert_eq!(back, variant);
        }
    }

    #[test]
    fn trust_level_serde_roundtrip() {
        for (variant, expected_str) in [
            (TrustLevel::Full, "\"full\""),
            (TrustLevel::None, "\"none\""),
        ] {
            let json = serde_json::to_string(&variant).unwrap();
            assert_eq!(json, expected_str);
            let back: TrustLevel = serde_json::from_str(&json).unwrap();
            assert_eq!(back, variant);
        }
    }

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
            topics: Some(vec!["math".to_string(), "algebra".to_string()]),
        }
    }

    #[test]
    fn publication_serde_roundtrip() {
        let pub1 = sample_publication();
        let json = serde_json::to_string(&pub1).unwrap();
        assert!(json.contains("publishedAt"));
        assert!(!json.contains("published_at"));
        let back: Publication = serde_json::from_str(&json).unwrap();
        assert_eq!(back.published_at, pub1.published_at);
        assert_eq!(back.topics, pub1.topics);

        // Test with topics = None
        let pub2 = Publication {
            visibility: Visibility::Private,
            published_at: 0,
            topics: None,
        };
        let json2 = serde_json::to_string(&pub2).unwrap();
        let back2: Publication = serde_json::from_str(&json2).unwrap();
        assert!(back2.topics.is_none());
    }

    #[test]
    fn signature_block_serde_roundtrip() {
        let sig = sample_signature_block();
        let json = serde_json::to_string(&sig).unwrap();
        assert!(json.contains("nodeId"));
        assert!(json.contains("publicKey"));
        assert!(!json.contains("node_id"));
        assert!(!json.contains("public_key"));
        let back: SignatureBlock = serde_json::from_str(&json).unwrap();
        assert_eq!(back.algorithm, "ed25519");
        assert_eq!(back.node_id, sig.node_id);
        assert_eq!(back.public_key, sig.public_key);
        assert_eq!(back.timestamp, sig.timestamp);
        assert_eq!(back.sig, sig.sig);
    }

    #[test]
    fn signed_lesson_serde_roundtrip() {
        let sl = SignedLesson {
            lesson: serde_json::json!({"id": "lesson-1", "title": "Intro", "extra_field": 42}),
            publication: sample_publication(),
            signature: sample_signature_block(),
        };
        let json = serde_json::to_string(&sl).unwrap();
        let back: SignedLesson = serde_json::from_str(&json).unwrap();
        assert_eq!(back.lesson["id"], "lesson-1");
        assert_eq!(back.lesson["extra_field"], 42);
        assert_eq!(back.publication.published_at, sl.publication.published_at);
    }

    #[test]
    fn signed_checkpoint_serde_roundtrip() {
        let sc = SignedCheckpoint {
            checkpoint: serde_json::json!({"id": "cp-1", "score": 95}),
            publication: sample_publication(),
            signature: sample_signature_block(),
        };
        let json = serde_json::to_string(&sc).unwrap();
        let back: SignedCheckpoint = serde_json::from_str(&json).unwrap();
        assert_eq!(back.checkpoint["id"], "cp-1");
        assert_eq!(back.checkpoint["score"], 95);
    }

    #[test]
    fn peer_connection_serde_roundtrip() {
        let pc = PeerConnection {
            node_id: "peer-1".to_string(),
            endpoint: "https://example.com".to_string(),
            trust_level: TrustLevel::Full,
            last_seen: Some(1700000000000),
            connected_since: None,
        };
        let json = serde_json::to_string(&pc).unwrap();
        assert!(json.contains("nodeId"));
        assert!(json.contains("trustLevel"));
        assert!(json.contains("lastSeen"));
        assert!(json.contains("connectedSince"));
        let back: PeerConnection = serde_json::from_str(&json).unwrap();
        assert_eq!(back.node_id, "peer-1");
        assert_eq!(back.trust_level, TrustLevel::Full);
        assert!(back.last_seen.is_some());
        assert!(back.connected_since.is_none());
    }

    #[test]
    fn revocation_serde_roundtrip() {
        let rev = Revocation {
            record_type: RecordType::Lesson,
            record_id: "lesson-1".to_string(),
            node_id: "node-abc123".to_string(),
            revoked_at: 1700000000000,
            reason: Some("Content outdated".to_string()),
            signature: Some(sample_signature_block()),
        };
        let json = serde_json::to_string(&rev).unwrap();
        assert!(json.contains("recordType"));
        assert!(json.contains("recordId"));
        assert!(json.contains("revokedAt"));
        let back: Revocation = serde_json::from_str(&json).unwrap();
        assert_eq!(back.record_type, RecordType::Lesson);
        assert_eq!(back.record_id, "lesson-1");
        assert_eq!(back.reason, Some("Content outdated".to_string()));
        assert!(back.signature.is_some());

        // Test with no reason and no signature
        let rev2 = Revocation {
            record_type: RecordType::Checkpoint,
            record_id: "cp-1".to_string(),
            node_id: "node-xyz".to_string(),
            revoked_at: 0,
            reason: None,
            signature: None,
        };
        let json2 = serde_json::to_string(&rev2).unwrap();
        let back2: Revocation = serde_json::from_str(&json2).unwrap();
        assert_eq!(back2.record_type, RecordType::Checkpoint);
        assert!(back2.reason.is_none());
        assert!(back2.signature.is_none());
    }

    #[test]
    fn publication_announcement_serde_roundtrip() {
        let ann = PublicationAnnouncement {
            record_type: RecordType::Lesson,
            signed_lesson: Some(SignedLesson {
                lesson: serde_json::json!({"id": "l1", "content": "test"}),
                publication: sample_publication(),
                signature: sample_signature_block(),
            }),
            signed_checkpoint: None,
        };
        let json = serde_json::to_string(&ann).unwrap();
        assert!(json.contains("recordType"));
        assert!(json.contains("signedLesson"));
        let back: PublicationAnnouncement = serde_json::from_str(&json).unwrap();
        assert_eq!(back.record_type, RecordType::Lesson);
        assert!(back.signed_lesson.is_some());
        assert!(back.signed_checkpoint.is_none());
    }

    #[test]
    fn revocation_announcement_serde_roundtrip() {
        let ann = RevocationAnnouncement {
            revocation: Revocation {
                record_type: RecordType::Lesson,
                record_id: "l1".to_string(),
                node_id: "node-abc".to_string(),
                revoked_at: 1700000000000,
                reason: Some("outdated".to_string()),
                signature: None,
            },
        };
        let json = serde_json::to_string(&ann).unwrap();
        let back: RevocationAnnouncement = serde_json::from_str(&json).unwrap();
        assert_eq!(back.revocation.record_id, "l1");
    }

    #[test]
    fn search_request_serde_roundtrip() {
        let req = SearchRequest {
            query: "rust ownership".to_string(),
            record_types: Some(vec![RecordType::Lesson]),
            filters: Some(SearchFilters {
                tags: Some(vec!["rust".to_string()]),
                topics: None,
                since: Some(1700000000000),
            }),
            limit: Some(10),
            request_id: "req-1".to_string(),
            origin: "node-abc".to_string(),
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("recordTypes"));
        assert!(json.contains("requestId"));
        let back: SearchRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(back.query, "rust ownership");
        assert_eq!(back.limit, Some(10));
        assert_eq!(back.request_id, "req-1");
        assert!(back.filters.is_some());
        let filters = back.filters.unwrap();
        assert_eq!(filters.tags, Some(vec!["rust".to_string()]));
        assert!(filters.topics.is_none());
    }

    #[test]
    fn search_response_serde_roundtrip() {
        let resp = SearchResponse {
            request_id: "req-1".to_string(),
            results: vec![SearchResult {
                record_type: RecordType::Lesson,
                signed_lesson: Some(SignedLesson {
                    lesson: serde_json::json!({"id": "l1"}),
                    publication: sample_publication(),
                    signature: sample_signature_block(),
                }),
                signed_checkpoint: None,
                score: 0.95,
                trust_score: 0.8,
                via: Some("node-xyz".to_string()),
            }],
            truncated: false,
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("requestId"));
        assert!(json.contains("trustScore"));
        let back: SearchResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(back.request_id, "req-1");
        assert_eq!(back.results.len(), 1);
        assert!(!back.truncated);
        let result = &back.results[0];
        assert_eq!(result.record_type, RecordType::Lesson);
        assert!((result.score - 0.95).abs() < f64::EPSILON);
        assert_eq!(result.via, Some("node-xyz".to_string()));
    }

    #[test]
    fn search_filters_serde_roundtrip() {
        let filters = SearchFilters {
            tags: Some(vec!["math".to_string()]),
            topics: Some(vec!["algebra".to_string()]),
            since: None,
        };
        let json = serde_json::to_string(&filters).unwrap();
        let back: SearchFilters = serde_json::from_str(&json).unwrap();
        assert_eq!(back.tags, Some(vec!["math".to_string()]));
        assert_eq!(back.topics, Some(vec!["algebra".to_string()]));
        assert!(back.since.is_none());
    }
}
