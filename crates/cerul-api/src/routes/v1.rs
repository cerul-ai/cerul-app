use axum::{
    routing::{get, post},
    Router,
};

use crate::ApiState;

mod models;

pub(crate) use models::*;

pub(crate) const API_PATHS: &[(&str, &[&str])] = &[
    ("/v1/status", &["get"]),
    ("/v1/openapi.json", &["get"]),
    ("/v1/search", &["post"]),
    ("/v1/ask", &["post"]),
    ("/v1/items", &["get"]),
    ("/v1/items/{id}", &["get"]),
    ("/v1/items/{id}/chunks", &["get"]),
    ("/v1/chunks/{id}/frame", &["get"]),
    ("/v1/chunks/{id}/video-segment", &["get"]),
    ("/v1/chunks/{id}/video-clip", &["get"]),
];

pub(crate) fn router() -> Router<ApiState> {
    Router::new()
        .route("/status", get(crate::v1_status))
        .route("/openapi.json", get(crate::v1_openapi_json))
        .route("/search", post(crate::v1_search))
        .route("/ask", post(crate::v1_ask))
        .route("/items", get(crate::v1_list_items))
        .route("/items/:id", get(crate::v1_get_item))
        .route("/items/:id/chunks", get(crate::v1_list_item_chunks))
        .route("/chunks/:id/frame", get(crate::get_chunk_frame))
        .route(
            "/chunks/:id/video-segment",
            get(crate::get_chunk_video_segment),
        )
        .route("/chunks/:id/video-clip", get(crate::get_chunk_video_clip))
}
