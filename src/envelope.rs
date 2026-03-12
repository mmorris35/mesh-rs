use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use chrono::Utc;
use ed25519_dalek::{Signature, Signer, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};

use crate::error::{MeshError, MeshResult};
use crate::identity::NodeIdentity;
use crate::signing::canonicalize;

/// A signed envelope for all MESH protocol messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MeshEnvelope {
    pub version: String,
    #[serde(rename = "type")]
    pub msg_type: String,
    pub timestamp: i64,
    pub sender: String,
    pub payload: serde_json::Value,
    pub signature: Option<String>,
}

impl MeshEnvelope {
    /// Create a new unsigned envelope with version "mesh/1.0" and current timestamp.
    pub fn new(sender: &str, msg_type: &str, payload: serde_json::Value) -> Self {
        Self {
            version: "mesh/1.0".to_string(),
            msg_type: msg_type.to_string(),
            timestamp: Utc::now().timestamp_millis(),
            sender: sender.to_string(),
            payload,
            signature: None,
        }
    }

    /// Sign this envelope using the given node identity.
    ///
    /// Serializes the envelope to a JSON value, removes the "signature" key,
    /// canonicalizes, signs with Ed25519, and stores the base64-encoded signature.
    pub fn sign(&mut self, identity: &NodeIdentity) -> MeshResult<()> {
        let mut value = serde_json::to_value(&*self)?;
        value.as_object_mut().unwrap().remove("signature");
        let canonical = canonicalize(&value)?;
        let sig = identity.signing_key().sign(canonical.as_bytes());
        self.signature = Some(BASE64.encode(sig.to_bytes()));
        Ok(())
    }

    /// Verify the envelope signature against a known verifying key.
    ///
    /// Returns `Err(MeshError::InvalidSignature)` if the signature is missing
    /// or does not match the canonical envelope content.
    pub fn verify_signature_with_key(&self, key: &VerifyingKey) -> MeshResult<()> {
        let sig_b64 = self.signature.as_ref().ok_or(MeshError::InvalidSignature)?;

        let mut value = serde_json::to_value(self)?;
        value.as_object_mut().unwrap().remove("signature");
        let canonical = canonicalize(&value)?;

        let sig_bytes = BASE64
            .decode(sig_b64)
            .map_err(|_| MeshError::InvalidSignature)?;
        let sig_array: [u8; 64] = sig_bytes
            .try_into()
            .map_err(|_| MeshError::InvalidSignature)?;
        let signature = Signature::from_bytes(&sig_array);

        key.verify(canonical.as_bytes(), &signature)
            .map_err(|_| MeshError::InvalidSignature)?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn new_envelope_has_correct_version() {
        let env = MeshEnvelope::new("sender-1", "publish", json!({"foo": "bar"}));
        assert_eq!(env.version, "mesh/1.0");
    }

    #[test]
    fn new_envelope_has_no_signature() {
        let env = MeshEnvelope::new("sender-1", "publish", json!({}));
        assert!(env.signature.is_none());
    }

    #[test]
    fn envelope_serializes_to_correct_json_structure() {
        let env = MeshEnvelope::new("sender-1", "search", json!({"q": "rust"}));
        let value = serde_json::to_value(&env).unwrap();
        let obj = value.as_object().unwrap();

        assert!(obj.contains_key("version"));
        assert!(obj.contains_key("type"));
        assert!(obj.contains_key("timestamp"));
        assert!(obj.contains_key("sender"));
        assert!(obj.contains_key("payload"));
        // "type" field should be used instead of "msgType"
        assert!(!obj.contains_key("msgType"));
        assert_eq!(value["version"], "mesh/1.0");
        assert_eq!(value["type"], "search");
        assert_eq!(value["sender"], "sender-1");
    }

    #[test]
    fn envelope_serde_roundtrip() {
        let env = MeshEnvelope::new("node-abc", "publish", json!({"data": [1, 2, 3]}));
        let json = serde_json::to_string(&env).unwrap();
        let back: MeshEnvelope = serde_json::from_str(&json).unwrap();
        assert_eq!(back.version, env.version);
        assert_eq!(back.msg_type, env.msg_type);
        assert_eq!(back.timestamp, env.timestamp);
        assert_eq!(back.sender, env.sender);
        assert_eq!(back.payload, env.payload);
        assert_eq!(back.signature, env.signature);
    }

    #[test]
    fn sign_and_verify_roundtrip() {
        let identity = NodeIdentity::generate();
        let mut env = MeshEnvelope::new(identity.node_id(), "publish", json!({"lesson": "hello"}));
        env.sign(&identity).unwrap();
        assert!(env.signature.is_some());

        env.verify_signature_with_key(identity.verifying_key())
            .unwrap();
    }

    #[test]
    fn verify_fails_without_signature() {
        let identity = NodeIdentity::generate();
        let env = MeshEnvelope::new(identity.node_id(), "search", json!({}));
        let result = env.verify_signature_with_key(identity.verifying_key());
        assert!(result.is_err());
    }

    #[test]
    fn verify_fails_with_wrong_key() {
        let identity1 = NodeIdentity::generate();
        let identity2 = NodeIdentity::generate();
        let mut env = MeshEnvelope::new(identity1.node_id(), "publish", json!({}));
        env.sign(&identity1).unwrap();

        let result = env.verify_signature_with_key(identity2.verifying_key());
        assert!(result.is_err());
    }

    #[test]
    fn verify_fails_after_tampering() {
        let identity = NodeIdentity::generate();
        let mut env = MeshEnvelope::new(identity.node_id(), "publish", json!({"a": 1}));
        env.sign(&identity).unwrap();

        env.payload = json!({"a": 2});
        let result = env.verify_signature_with_key(identity.verifying_key());
        assert!(result.is_err());
    }

    #[test]
    fn version_always_mesh_1_0() {
        for _ in 0..5 {
            let env = MeshEnvelope::new("x", "y", json!(null));
            assert_eq!(env.version, "mesh/1.0");
        }
    }
}
