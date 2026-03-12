use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};
use std::fmt;

/// All possible errors in the MESH federation protocol.
#[derive(Debug)]
pub enum MeshError {
    /// Signature verification failed.
    InvalidSignature,
    /// No trust path to the given node ID.
    UntrustedNode(String),
    /// Record ID not found.
    UnknownRecord(String),
    /// Record has already been revoked.
    AlreadyRevoked(String),
    /// Too many requests.
    RateLimited,
    /// Malformed request with detail message.
    InvalidRequest(String),
    /// Database / storage error.
    StorageError(String),
    /// HTTP / connectivity error.
    NetworkError(String),
    /// Keypair / identity issues.
    IdentityError(String),
    /// JSON / canonical serialization error.
    SerializationError(String),
}

impl fmt::Display for MeshError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MeshError::InvalidSignature => write!(f, "Signature verification failed"),
            MeshError::UntrustedNode(id) => write!(f, "No trust path to node: {id}"),
            MeshError::UnknownRecord(id) => write!(f, "Record not found: {id}"),
            MeshError::AlreadyRevoked(id) => write!(f, "Record already revoked: {id}"),
            MeshError::RateLimited => write!(f, "Rate limited"),
            MeshError::InvalidRequest(detail) => write!(f, "Invalid request: {detail}"),
            MeshError::StorageError(detail) => write!(f, "Storage error: {detail}"),
            MeshError::NetworkError(detail) => write!(f, "Network error: {detail}"),
            MeshError::IdentityError(detail) => write!(f, "Identity error: {detail}"),
            MeshError::SerializationError(detail) => write!(f, "Serialization error: {detail}"),
        }
    }
}

impl std::error::Error for MeshError {}

impl From<rusqlite::Error> for MeshError {
    fn from(err: rusqlite::Error) -> Self {
        MeshError::StorageError(err.to_string())
    }
}

impl From<serde_json::Error> for MeshError {
    fn from(err: serde_json::Error) -> Self {
        MeshError::SerializationError(err.to_string())
    }
}

impl From<reqwest::Error> for MeshError {
    fn from(err: reqwest::Error) -> Self {
        MeshError::NetworkError(err.to_string())
    }
}

/// Convenience type alias for results throughout the MESH crate.
pub type MeshResult<T> = Result<T, MeshError>;

/// JSON error response body per spec section 9.1.
#[derive(Debug, Serialize, Deserialize)]
pub struct ErrorResponse {
    pub error: bool,
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

impl MeshError {
    /// Returns the spec error code string for this variant.
    pub fn error_code(&self) -> &'static str {
        match self {
            MeshError::InvalidSignature => "INVALID_SIGNATURE",
            MeshError::UntrustedNode(_) => "UNTRUSTED_NODE",
            MeshError::UnknownRecord(_) => "UNKNOWN_RECORD",
            MeshError::AlreadyRevoked(_) => "ALREADY_REVOKED",
            MeshError::RateLimited => "RATE_LIMITED",
            MeshError::InvalidRequest(_) => "INVALID_REQUEST",
            MeshError::StorageError(_) => "STORAGE_ERROR",
            MeshError::NetworkError(_) => "NETWORK_ERROR",
            MeshError::IdentityError(_) => "IDENTITY_ERROR",
            MeshError::SerializationError(_) => "SERIALIZATION_ERROR",
        }
    }

    /// Returns the HTTP status code for this variant.
    pub fn status_code(&self) -> StatusCode {
        match self {
            MeshError::InvalidSignature => StatusCode::UNAUTHORIZED,
            MeshError::UntrustedNode(_) => StatusCode::FORBIDDEN,
            MeshError::UnknownRecord(_) => StatusCode::NOT_FOUND,
            MeshError::AlreadyRevoked(_) => StatusCode::CONFLICT,
            MeshError::RateLimited => StatusCode::TOO_MANY_REQUESTS,
            MeshError::InvalidRequest(_) => StatusCode::BAD_REQUEST,
            MeshError::StorageError(_) => StatusCode::INTERNAL_SERVER_ERROR,
            MeshError::NetworkError(_) => StatusCode::BAD_GATEWAY,
            MeshError::IdentityError(_) => StatusCode::INTERNAL_SERVER_ERROR,
            MeshError::SerializationError(_) => StatusCode::BAD_REQUEST,
        }
    }
}

impl IntoResponse for MeshError {
    fn into_response(self) -> Response {
        let status = self.status_code();
        let body = ErrorResponse {
            error: true,
            code: self.error_code().to_string(),
            message: self.to_string(),
            details: None,
        };
        (status, Json(body)).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_each_variant() {
        assert_eq!(
            MeshError::InvalidSignature.to_string(),
            "Signature verification failed"
        );
        assert_eq!(
            MeshError::UntrustedNode("node-1".into()).to_string(),
            "No trust path to node: node-1"
        );
        assert_eq!(
            MeshError::UnknownRecord("rec-1".into()).to_string(),
            "Record not found: rec-1"
        );
        assert_eq!(
            MeshError::AlreadyRevoked("rec-2".into()).to_string(),
            "Record already revoked: rec-2"
        );
        assert_eq!(MeshError::RateLimited.to_string(), "Rate limited");
        assert_eq!(
            MeshError::InvalidRequest("bad field".into()).to_string(),
            "Invalid request: bad field"
        );
        assert_eq!(
            MeshError::StorageError("disk full".into()).to_string(),
            "Storage error: disk full"
        );
        assert_eq!(
            MeshError::NetworkError("timeout".into()).to_string(),
            "Network error: timeout"
        );
        assert_eq!(
            MeshError::IdentityError("missing key".into()).to_string(),
            "Identity error: missing key"
        );
        assert_eq!(
            MeshError::SerializationError("invalid json".into()).to_string(),
            "Serialization error: invalid json"
        );
    }

    #[test]
    fn error_code_mapping() {
        assert_eq!(
            MeshError::InvalidSignature.error_code(),
            "INVALID_SIGNATURE"
        );
        assert_eq!(
            MeshError::UntrustedNode("x".into()).error_code(),
            "UNTRUSTED_NODE"
        );
        assert_eq!(
            MeshError::UnknownRecord("x".into()).error_code(),
            "UNKNOWN_RECORD"
        );
        assert_eq!(
            MeshError::AlreadyRevoked("x".into()).error_code(),
            "ALREADY_REVOKED"
        );
        assert_eq!(MeshError::RateLimited.error_code(), "RATE_LIMITED");
        assert_eq!(
            MeshError::InvalidRequest("x".into()).error_code(),
            "INVALID_REQUEST"
        );
        assert_eq!(
            MeshError::StorageError("x".into()).error_code(),
            "STORAGE_ERROR"
        );
        assert_eq!(
            MeshError::NetworkError("x".into()).error_code(),
            "NETWORK_ERROR"
        );
        assert_eq!(
            MeshError::IdentityError("x".into()).error_code(),
            "IDENTITY_ERROR"
        );
        assert_eq!(
            MeshError::SerializationError("x".into()).error_code(),
            "SERIALIZATION_ERROR"
        );
    }

    #[test]
    fn http_status_mapping() {
        assert_eq!(
            MeshError::InvalidSignature.status_code(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            MeshError::UntrustedNode("x".into()).status_code(),
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            MeshError::UnknownRecord("x".into()).status_code(),
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            MeshError::AlreadyRevoked("x".into()).status_code(),
            StatusCode::CONFLICT
        );
        assert_eq!(
            MeshError::RateLimited.status_code(),
            StatusCode::TOO_MANY_REQUESTS
        );
        assert_eq!(
            MeshError::InvalidRequest("x".into()).status_code(),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            MeshError::StorageError("x".into()).status_code(),
            StatusCode::INTERNAL_SERVER_ERROR
        );
        assert_eq!(
            MeshError::NetworkError("x".into()).status_code(),
            StatusCode::BAD_GATEWAY
        );
        assert_eq!(
            MeshError::IdentityError("x".into()).status_code(),
            StatusCode::INTERNAL_SERVER_ERROR
        );
        assert_eq!(
            MeshError::SerializationError("x".into()).status_code(),
            StatusCode::BAD_REQUEST
        );
    }

    #[test]
    fn from_rusqlite_error() {
        let sqlite_err = rusqlite::Error::QueryReturnedNoRows;
        let mesh_err: MeshError = sqlite_err.into();
        assert!(matches!(mesh_err, MeshError::StorageError(_)));
        assert!(mesh_err.to_string().contains("Storage error"));
    }

    #[test]
    fn from_serde_json_error() {
        let json_err = serde_json::from_str::<serde_json::Value>("{{bad json").unwrap_err();
        let mesh_err: MeshError = json_err.into();
        assert!(matches!(mesh_err, MeshError::SerializationError(_)));
        assert!(mesh_err.to_string().contains("Serialization error"));
    }

    #[test]
    fn from_reqwest_error() {
        // Build a reqwest error by trying to parse an invalid URL
        let reqwest_err = reqwest::Client::new().get("://").build().unwrap_err();
        let mesh_err: MeshError = reqwest_err.into();
        assert!(matches!(mesh_err, MeshError::NetworkError(_)));
        assert!(mesh_err.to_string().contains("Network error"));
    }

    #[test]
    fn error_response_serializes_correctly() {
        let resp = ErrorResponse {
            error: true,
            code: "INVALID_SIGNATURE".to_string(),
            message: "Signature verification failed".to_string(),
            details: None,
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["error"], true);
        assert_eq!(json["code"], "INVALID_SIGNATURE");
        assert_eq!(json["message"], "Signature verification failed");
        assert!(json.get("details").is_none());

        let resp_with_details = ErrorResponse {
            error: true,
            code: "INVALID_REQUEST".to_string(),
            message: "Invalid request: missing field".to_string(),
            details: Some(serde_json::json!({"field": "name"})),
        };
        let json = serde_json::to_value(&resp_with_details).unwrap();
        assert_eq!(json["details"]["field"], "name");
    }

    #[test]
    fn error_response_deserializes() {
        let json_str = r#"{"error":true,"code":"RATE_LIMITED","message":"Rate limited"}"#;
        let resp: ErrorResponse = serde_json::from_str(json_str).unwrap();
        assert!(resp.error);
        assert_eq!(resp.code, "RATE_LIMITED");
        assert_eq!(resp.message, "Rate limited");
        assert!(resp.details.is_none());
    }

    #[test]
    fn mesh_error_is_std_error() {
        let err: Box<dyn std::error::Error> = Box::new(MeshError::InvalidSignature);
        assert_eq!(err.to_string(), "Signature verification failed");
    }
}
