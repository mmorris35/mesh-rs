use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use chrono::Utc;
use ed25519_dalek::{Signature, Signer, Verifier, VerifyingKey};
use serde_json::json;

use crate::error::{MeshError, MeshResult};
use crate::identity::NodeIdentity;
use crate::types::*;

/// Produce RFC 8785 canonical JSON from a `serde_json::Value`.
///
/// Keys are sorted lexicographically, no extraneous whitespace is emitted,
/// and numbers use the shortest possible representation.
pub fn canonicalize(value: &serde_json::Value) -> MeshResult<String> {
    json_canon::to_string(value).map_err(|e| MeshError::SerializationError(e.to_string()))
}

/// Sign a lesson record, producing a `SignedLesson`.
///
/// Constructs a signable payload from the lesson, publication, and current
/// timestamp, canonicalizes it, and signs with the node's Ed25519 key.
pub fn sign_lesson(
    identity: &NodeIdentity,
    lesson: &serde_json::Value,
    publication: &Publication,
) -> MeshResult<SignedLesson> {
    let timestamp = Utc::now().timestamp_millis();
    let pub_value = serde_json::to_value(publication)
        .map_err(|e| MeshError::SerializationError(e.to_string()))?;
    let payload = json!({
        "lesson": lesson,
        "publication": pub_value,
        "timestamp": timestamp,
    });
    let canonical = canonicalize(&payload)?;
    let signature = identity.signing_key().sign(canonical.as_bytes());

    let sig_block = SignatureBlock {
        algorithm: "ed25519".to_string(),
        node_id: identity.node_id().to_string(),
        public_key: identity.public_key_base64(),
        timestamp,
        sig: BASE64.encode(signature.to_bytes()),
    };

    Ok(SignedLesson {
        lesson: lesson.clone(),
        publication: publication.clone(),
        signature: sig_block,
    })
}

/// Sign a checkpoint record, producing a `SignedCheckpoint`.
pub fn sign_checkpoint(
    identity: &NodeIdentity,
    checkpoint: &serde_json::Value,
    publication: &Publication,
) -> MeshResult<SignedCheckpoint> {
    let timestamp = Utc::now().timestamp_millis();
    let pub_value = serde_json::to_value(publication)
        .map_err(|e| MeshError::SerializationError(e.to_string()))?;
    let payload = json!({
        "checkpoint": checkpoint,
        "publication": pub_value,
        "timestamp": timestamp,
    });
    let canonical = canonicalize(&payload)?;
    let signature = identity.signing_key().sign(canonical.as_bytes());

    let sig_block = SignatureBlock {
        algorithm: "ed25519".to_string(),
        node_id: identity.node_id().to_string(),
        public_key: identity.public_key_base64(),
        timestamp,
        sig: BASE64.encode(signature.to_bytes()),
    };

    Ok(SignedCheckpoint {
        checkpoint: checkpoint.clone(),
        publication: publication.clone(),
        signature: sig_block,
    })
}

/// Verify the Ed25519 signature on a `SignedLesson`.
///
/// Reconstructs the signable payload, canonicalizes it, and verifies
/// the signature against the embedded public key. Also checks that
/// `node_id` matches the base58-encoded public key.
pub fn verify_signed_lesson(signed: &SignedLesson) -> MeshResult<()> {
    let pub_value = serde_json::to_value(&signed.publication)
        .map_err(|e| MeshError::SerializationError(e.to_string()))?;
    let payload = json!({
        "lesson": &signed.lesson,
        "publication": pub_value,
        "timestamp": signed.signature.timestamp,
    });
    verify_payload_signature(&payload, &signed.signature)
}

/// Verify the Ed25519 signature on a `SignedCheckpoint`.
pub fn verify_signed_checkpoint(signed: &SignedCheckpoint) -> MeshResult<()> {
    let pub_value = serde_json::to_value(&signed.publication)
        .map_err(|e| MeshError::SerializationError(e.to_string()))?;
    let payload = json!({
        "checkpoint": &signed.checkpoint,
        "publication": pub_value,
        "timestamp": signed.signature.timestamp,
    });
    verify_payload_signature(&payload, &signed.signature)
}

/// Sign a revocation in place, setting its `signature` field.
///
/// Serializes the revocation to JSON, removes the `signature` key,
/// canonicalizes, signs, and writes back the signature block.
pub fn sign_revocation(identity: &NodeIdentity, revocation: &mut Revocation) -> MeshResult<()> {
    let timestamp = Utc::now().timestamp_millis();
    let mut rev_value = serde_json::to_value(&*revocation)
        .map_err(|e| MeshError::SerializationError(e.to_string()))?;
    rev_value.as_object_mut().unwrap().remove("signature");
    let canonical = canonicalize(&rev_value)?;
    let signature = identity.signing_key().sign(canonical.as_bytes());

    revocation.signature = Some(SignatureBlock {
        algorithm: "ed25519".to_string(),
        node_id: identity.node_id().to_string(),
        public_key: identity.public_key_base64(),
        timestamp,
        sig: BASE64.encode(signature.to_bytes()),
    });

    Ok(())
}

/// Verify the signature on a `Revocation`.
///
/// Returns `Err(MeshError::InvalidSignature)` if the revocation has no
/// signature block or if verification fails.
pub fn verify_revocation(revocation: &Revocation) -> MeshResult<()> {
    let sig_block = revocation
        .signature
        .as_ref()
        .ok_or(MeshError::InvalidSignature)?;

    let mut rev_value = serde_json::to_value(revocation)
        .map_err(|e| MeshError::SerializationError(e.to_string()))?;
    rev_value.as_object_mut().unwrap().remove("signature");
    let canonical = canonicalize(&rev_value)?;

    verify_raw_signature(canonical.as_bytes(), sig_block)
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Verify a canonicalized payload against a `SignatureBlock`.
fn verify_payload_signature(
    payload: &serde_json::Value,
    sig_block: &SignatureBlock,
) -> MeshResult<()> {
    let canonical = canonicalize(payload)?;
    verify_raw_signature(canonical.as_bytes(), sig_block)
}

/// Core verification: decode public key and signature from base64,
/// verify the Ed25519 signature, and check node_id consistency.
fn verify_raw_signature(message: &[u8], sig_block: &SignatureBlock) -> MeshResult<()> {
    // Decode public key
    let pk_bytes = BASE64
        .decode(&sig_block.public_key)
        .map_err(|_| MeshError::InvalidSignature)?;
    let pk_array: [u8; 32] = pk_bytes
        .try_into()
        .map_err(|_| MeshError::InvalidSignature)?;
    let verifying_key =
        VerifyingKey::from_bytes(&pk_array).map_err(|_| MeshError::InvalidSignature)?;

    // Decode signature
    let sig_bytes = BASE64
        .decode(&sig_block.sig)
        .map_err(|_| MeshError::InvalidSignature)?;
    let sig_array: [u8; 64] = sig_bytes
        .try_into()
        .map_err(|_| MeshError::InvalidSignature)?;
    let signature = Signature::from_bytes(&sig_array);

    // Verify Ed25519 signature
    verifying_key
        .verify(message, &signature)
        .map_err(|_| MeshError::InvalidSignature)?;

    // Verify node_id matches public key
    let expected_node_id = bs58::encode(pk_array).into_string();
    if sig_block.node_id != expected_node_id {
        return Err(MeshError::InvalidSignature);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn test_publication() -> Publication {
        Publication {
            visibility: Visibility::Public,
            published_at: 1_700_000_000_000,
            topics: Some(vec!["rust".to_string()]),
        }
    }

    #[test]
    fn keys_sorted_lexicographically() {
        let value = json!({"z": 1, "a": 2, "m": 3});
        let result = canonicalize(&value).unwrap();
        assert_eq!(result, r#"{"a":2,"m":3,"z":1}"#);
    }

    #[test]
    fn no_whitespace_in_output() {
        let value = json!({"key": "value", "num": 42});
        let result = canonicalize(&value).unwrap();
        assert!(!result.contains(' '));
        assert!(!result.contains('\n'));
        assert!(!result.contains('\t'));
    }

    #[test]
    fn numbers_in_shortest_form() {
        let value = json!(1.0);
        let result = canonicalize(&value).unwrap();
        assert_eq!(result, "1");

        let value = json!(1.5);
        let result = canonicalize(&value).unwrap();
        assert_eq!(result, "1.5");
    }

    #[test]
    fn nested_objects_recursively_canonicalized() {
        let value = json!({
            "outer_z": {"inner_b": 2, "inner_a": 1},
            "outer_a": {"inner_z": 26, "inner_m": 13}
        });
        let result = canonicalize(&value).unwrap();
        assert_eq!(
            result,
            r#"{"outer_a":{"inner_m":13,"inner_z":26},"outer_z":{"inner_a":1,"inner_b":2}}"#
        );
    }

    #[test]
    fn complex_lesson_with_graph_metadata() {
        let value = json!({
            "lesson": {
                "id": "l1",
                "content": "test",
                "tags": ["rust"],
                "solved_problem": "memory"
            },
            "publication": {
                "visibility": "public",
                "publishedAt": 1234567890
            },
            "timestamp": 1234567890
        });
        let result = canonicalize(&value).unwrap();

        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert!(parsed.is_object());

        assert_eq!(
            result,
            r#"{"lesson":{"content":"test","id":"l1","solved_problem":"memory","tags":["rust"]},"publication":{"publishedAt":1234567890,"visibility":"public"},"timestamp":1234567890}"#
        );
    }

    #[test]
    fn determinism_same_input_same_output() {
        let value = json!({
            "lesson": {
                "id": "l1",
                "content": "test",
                "tags": ["rust"],
                "solved_problem": "memory"
            },
            "publication": {
                "visibility": "public",
                "publishedAt": 1234567890
            },
            "timestamp": 1234567890
        });
        let first = canonicalize(&value).unwrap();
        let second = canonicalize(&value).unwrap();
        assert_eq!(first, second);
    }

    // ----- Record signing & verification tests -----

    #[test]
    fn sign_and_verify_lesson() {
        let identity = NodeIdentity::generate();
        let lesson = json!({"id": "l1", "content": "Learn Rust ownership"});
        let pub_info = test_publication();

        let signed = sign_lesson(&identity, &lesson, &pub_info).unwrap();
        assert_eq!(signed.lesson, lesson);
        assert_eq!(signed.signature.algorithm, "ed25519");
        assert_eq!(signed.signature.node_id, identity.node_id());

        verify_signed_lesson(&signed).unwrap();
    }

    #[test]
    fn sign_and_verify_checkpoint() {
        let identity = NodeIdentity::generate();
        let checkpoint = json!({"id": "cp1", "score": 95});
        let pub_info = test_publication();

        let signed = sign_checkpoint(&identity, &checkpoint, &pub_info).unwrap();
        assert_eq!(signed.checkpoint, checkpoint);

        verify_signed_checkpoint(&signed).unwrap();
    }

    #[test]
    fn tamper_lesson_content_fails_verification() {
        let identity = NodeIdentity::generate();
        let lesson = json!({"id": "l1", "content": "original"});
        let pub_info = test_publication();

        let mut signed = sign_lesson(&identity, &lesson, &pub_info).unwrap();
        signed.lesson = json!({"id": "l1", "content": "tampered"});

        let result = verify_signed_lesson(&signed);
        assert!(result.is_err());
    }

    #[test]
    fn tamper_publication_fails_verification() {
        let identity = NodeIdentity::generate();
        let lesson = json!({"id": "l1", "content": "test"});
        let pub_info = test_publication();

        let mut signed = sign_lesson(&identity, &lesson, &pub_info).unwrap();
        signed.publication.visibility = Visibility::Private;

        let result = verify_signed_lesson(&signed);
        assert!(result.is_err());
    }

    #[test]
    fn wrong_key_fails_verification() {
        let identity1 = NodeIdentity::generate();
        let identity2 = NodeIdentity::generate();
        let lesson = json!({"id": "l1", "content": "test"});
        let pub_info = test_publication();

        let mut signed = sign_lesson(&identity1, &lesson, &pub_info).unwrap();
        // Replace the public key and node_id with identity2's values
        signed.signature.public_key = identity2.public_key_base64();
        signed.signature.node_id = identity2.node_id().to_string();

        let result = verify_signed_lesson(&signed);
        assert!(result.is_err());
    }

    #[test]
    fn sign_and_verify_revocation() {
        let identity = NodeIdentity::generate();
        let mut revocation = Revocation {
            record_type: RecordType::Lesson,
            record_id: "l1".to_string(),
            node_id: identity.node_id().to_string(),
            revoked_at: Utc::now().timestamp_millis(),
            reason: Some("outdated".to_string()),
            signature: None,
        };

        sign_revocation(&identity, &mut revocation).unwrap();
        assert!(revocation.signature.is_some());

        verify_revocation(&revocation).unwrap();
    }

    #[test]
    fn verify_revocation_without_signature_fails() {
        let revocation = Revocation {
            record_type: RecordType::Lesson,
            record_id: "l1".to_string(),
            node_id: "some-node".to_string(),
            revoked_at: 1_700_000_000_000,
            reason: None,
            signature: None,
        };

        let result = verify_revocation(&revocation);
        assert!(result.is_err());
    }

    #[test]
    fn node_id_public_key_mismatch_caught() {
        let identity = NodeIdentity::generate();
        let lesson = json!({"id": "l1", "content": "test"});
        let pub_info = test_publication();

        let mut signed = sign_lesson(&identity, &lesson, &pub_info).unwrap();
        // Keep original public_key (so sig verifies) but change node_id
        signed.signature.node_id = "bogus_node_id".to_string();

        let result = verify_signed_lesson(&signed);
        assert!(result.is_err());
    }

    #[test]
    fn extra_fields_covered_by_signature() {
        let identity = NodeIdentity::generate();
        let lesson = json!({
            "id": "l1",
            "content": "test",
            "graph_metadata": {"edges": [1, 2, 3]},
            "custom_field": "extra"
        });
        let pub_info = test_publication();

        let signed = sign_lesson(&identity, &lesson, &pub_info).unwrap();
        verify_signed_lesson(&signed).unwrap();

        // Tamper with extra field
        let mut tampered = signed;
        tampered.lesson["graph_metadata"] = json!({"edges": [1, 2, 4]});
        let result = verify_signed_lesson(&tampered);
        assert!(result.is_err());
    }
}
