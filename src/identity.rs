use crate::error::{MeshError, MeshResult};
use crate::signing::canonicalize;
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;

/// Represents a node's cryptographic identity in the MESH federation.
///
/// Holds an Ed25519 signing key (private) and its corresponding verifying key (public),
/// along with derived identifiers (base58 node ID and SHA-256 fingerprint).
pub struct NodeIdentity {
    signing_key: SigningKey,
    verifying_key: VerifyingKey,
    node_id: String,
    fingerprint: String,
}

impl NodeIdentity {
    /// Generate a new random Ed25519 keypair.
    pub fn generate() -> Self {
        let signing_key = SigningKey::generate(&mut OsRng);
        Self::from_signing_key(signing_key)
    }

    /// Reconstruct a `NodeIdentity` from stored private key bytes.
    pub fn from_private_key_bytes(bytes: &[u8; 32]) -> MeshResult<Self> {
        let signing_key = SigningKey::from_bytes(bytes);
        Ok(Self::from_signing_key(signing_key))
    }

    /// Internal helper to build from a `SigningKey`.
    fn from_signing_key(signing_key: SigningKey) -> Self {
        let verifying_key = signing_key.verifying_key();
        let pub_bytes = verifying_key.to_bytes();

        let node_id = bs58::encode(&pub_bytes).into_string();

        let mut hasher = Sha256::new();
        hasher.update(pub_bytes);
        let hash = hasher.finalize();
        let fingerprint = hash[..8]
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();

        Self {
            signing_key,
            verifying_key,
            node_id,
            fingerprint,
        }
    }

    /// Base58-encoded public key used as the node identifier.
    pub fn node_id(&self) -> &str {
        &self.node_id
    }

    /// First 16 hex characters of SHA-256(public_key).
    pub fn fingerprint(&self) -> &str {
        &self.fingerprint
    }

    /// Base64-encoded public key for wire format.
    pub fn public_key_base64(&self) -> String {
        BASE64.encode(self.verifying_key.to_bytes())
    }

    /// Raw private key bytes for secure storage. NEVER log this.
    pub fn private_key_bytes(&self) -> &[u8; 32] {
        self.signing_key.as_bytes()
    }

    /// Reference to the verifying (public) key for signature verification.
    pub fn verifying_key(&self) -> &VerifyingKey {
        &self.verifying_key
    }

    /// Reference to the signing (private) key for creating signatures.
    pub fn signing_key(&self) -> &SigningKey {
        &self.signing_key
    }

    /// Build a self-signed identity document for this node.
    ///
    /// The `mesh_endpoint` is the base URL at which this node's `/mesh` API is reachable.
    pub fn identity_document(&self, mesh_endpoint: &str) -> MeshResult<IdentityDocument> {
        let mut doc = IdentityDocument {
            version: "mesh/1.0".to_string(),
            node_id: self.node_id.clone(),
            public_key: self.public_key_base64(),
            endpoints: IdentityEndpoints {
                mesh: mesh_endpoint.to_string(),
            },
            capabilities: vec!["search".to_string()],
            signature: String::new(),
        };

        // Serialize to Value, remove signature, canonicalize, sign
        let mut value = serde_json::to_value(&doc)?;
        value.as_object_mut().unwrap().remove("signature");
        let canonical = canonicalize(&value)?;
        let sig = self.signing_key.sign(canonical.as_bytes());
        doc.signature = BASE64.encode(sig.to_bytes());

        Ok(doc)
    }
}

/// Endpoints advertised by a MESH node.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IdentityEndpoints {
    /// The `/mesh` base URL for this node.
    pub mesh: String,
}

/// A self-signed identity document that a MESH node publishes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IdentityDocument {
    pub version: String,
    pub node_id: String,
    pub public_key: String,
    pub endpoints: IdentityEndpoints,
    pub capabilities: Vec<String>,
    pub signature: String,
}

impl IdentityDocument {
    /// Verify the self-signature on this identity document.
    ///
    /// Checks that `node_id` matches `public_key` and that the Ed25519 signature
    /// over the canonical JSON (excluding the `signature` field) is valid.
    pub fn verify(&self) -> MeshResult<()> {
        // Decode the public key
        let pk_bytes = BASE64
            .decode(&self.public_key)
            .map_err(|e| MeshError::InvalidRequest(format!("invalid base64 public key: {e}")))?;
        let pk_array: [u8; 32] = pk_bytes
            .try_into()
            .map_err(|_| MeshError::InvalidRequest("public key must be 32 bytes".to_string()))?;
        let verifying_key = VerifyingKey::from_bytes(&pk_array)
            .map_err(|e| MeshError::InvalidRequest(format!("invalid Ed25519 key: {e}")))?;

        // Verify node_id == base58(public_key bytes)
        let expected_node_id = bs58::encode(&pk_array).into_string();
        if self.node_id != expected_node_id {
            return Err(MeshError::InvalidSignature);
        }

        // Reconstruct the canonical payload (without the signature field)
        let mut value = serde_json::to_value(self)?;
        value.as_object_mut().unwrap().remove("signature");
        let canonical = canonicalize(&value)?;

        // Decode and verify the signature
        let sig_bytes = BASE64
            .decode(&self.signature)
            .map_err(|e| MeshError::InvalidRequest(format!("invalid base64 signature: {e}")))?;
        let sig_array: [u8; 64] = sig_bytes
            .try_into()
            .map_err(|_| MeshError::InvalidRequest("signature must be 64 bytes".to_string()))?;
        let signature = Signature::from_bytes(&sig_array);

        verifying_key
            .verify(canonical.as_bytes(), &signature)
            .map_err(|_| MeshError::InvalidSignature)?;

        Ok(())
    }
}

impl fmt::Debug for NodeIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NodeIdentity")
            .field("node_id", &self.node_id)
            .field("fingerprint", &self.fingerprint)
            .field("signing_key", &"[REDACTED]")
            .field("verifying_key", &self.verifying_key)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_produces_valid_base58_node_id() {
        let identity = NodeIdentity::generate();
        let node_id = identity.node_id();
        assert!(!node_id.is_empty());
        // Verify it decodes as valid base58
        let decoded = bs58::decode(node_id).into_vec().unwrap();
        assert_eq!(decoded.len(), 32, "public key should be 32 bytes");
    }

    #[test]
    fn round_trip_private_key_bytes() {
        let identity1 = NodeIdentity::generate();
        let bytes = *identity1.private_key_bytes();
        let identity2 = NodeIdentity::from_private_key_bytes(&bytes).unwrap();
        assert_eq!(identity1.node_id(), identity2.node_id());
        assert_eq!(identity1.fingerprint(), identity2.fingerprint());
        assert_eq!(identity1.public_key_base64(), identity2.public_key_base64());
    }

    #[test]
    fn fingerprint_is_16_hex_chars() {
        let identity = NodeIdentity::generate();
        let fp = identity.fingerprint();
        assert_eq!(fp.len(), 16, "fingerprint should be 16 hex chars");
        assert!(
            fp.chars().all(|c| c.is_ascii_hexdigit()),
            "fingerprint should only contain hex characters"
        );
    }

    #[test]
    fn debug_redacts_private_key() {
        let identity = NodeIdentity::generate();
        let debug_output = format!("{:?}", identity);
        assert!(
            debug_output.contains("[REDACTED]"),
            "debug output should contain [REDACTED]"
        );
        // Ensure the raw private key bytes are not present
        let private_bytes = identity.private_key_bytes();
        let hex_private = private_bytes
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        assert!(
            !debug_output.contains(&hex_private),
            "debug output must NOT contain private key hex"
        );
    }

    #[test]
    fn two_generates_produce_different_keypairs() {
        let id1 = NodeIdentity::generate();
        let id2 = NodeIdentity::generate();
        assert_ne!(id1.node_id(), id2.node_id());
        assert_ne!(id1.fingerprint(), id2.fingerprint());
    }

    #[test]
    fn identity_document_self_signature_verifies() {
        let identity = NodeIdentity::generate();
        let doc = identity
            .identity_document("https://example.com/mesh")
            .unwrap();
        assert!(doc.verify().is_ok());
    }

    #[test]
    fn identity_document_tampered_node_id_fails_verification() {
        let identity = NodeIdentity::generate();
        let mut doc = identity
            .identity_document("https://example.com/mesh")
            .unwrap();
        doc.node_id = "TAMPERED_NODE_ID".to_string();
        assert!(doc.verify().is_err());
    }

    #[test]
    fn identity_document_node_id_matches_public_key() {
        let identity = NodeIdentity::generate();
        let doc = identity
            .identity_document("https://example.com/mesh")
            .unwrap();
        let pk_bytes = BASE64.decode(&doc.public_key).unwrap();
        let expected_node_id = bs58::encode(&pk_bytes).into_string();
        assert_eq!(doc.node_id, expected_node_id);
    }

    #[test]
    fn identity_document_serde_round_trip() {
        let identity = NodeIdentity::generate();
        let doc = identity
            .identity_document("https://example.com/mesh")
            .unwrap();
        let json = serde_json::to_string(&doc).unwrap();
        let deserialized: IdentityDocument = serde_json::from_str(&json).unwrap();
        let json2 = serde_json::to_string(&deserialized).unwrap();
        assert_eq!(json, json2);
        assert!(deserialized.verify().is_ok());
    }

    #[test]
    fn identity_document_version_is_mesh_1_0() {
        let identity = NodeIdentity::generate();
        let doc = identity
            .identity_document("https://example.com/mesh")
            .unwrap();
        assert_eq!(doc.version, "mesh/1.0");
    }

    #[test]
    fn identity_document_capabilities_contain_search() {
        let identity = NodeIdentity::generate();
        let doc = identity
            .identity_document("https://example.com/mesh")
            .unwrap();
        assert!(doc.capabilities.contains(&"search".to_string()));
    }
}
