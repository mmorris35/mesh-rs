pub mod announce;
pub mod identity;
pub mod peers;
pub mod search;

use std::sync::{Arc, Mutex};

use axum::routing::{get, post};
use axum::Router;

use crate::identity::NodeIdentity;

pub struct MeshState {
    pub identity: Arc<NodeIdentity>,
    pub db: Arc<Mutex<rusqlite::Connection>>,
    pub mesh_endpoint: String,
}

pub fn mesh_router(state: Arc<MeshState>) -> Router {
    Router::new()
        .route("/.well-known/mesh/identity", get(identity::get_identity))
        .route("/mesh/v1/identity", get(identity::get_identity))
        .route("/mesh/v1/announce", post(announce::post_announce))
        .route("/mesh/v1/search", post(search::post_search))
        .route("/mesh/v1/peers", get(peers::get_peers))
        .with_state(state)
}
