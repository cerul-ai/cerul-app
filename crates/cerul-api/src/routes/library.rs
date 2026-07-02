use axum::{
    routing::{get, post},
    Router,
};

use crate::{video_understanding, ApiState};

pub(crate) fn router() -> Router<ApiState> {
    Router::new()
        .route("/items", get(crate::list_items))
        .route(
            "/items/:id",
            get(crate::get_item)
                .patch(crate::update_item)
                .delete(crate::remove_item),
        )
        .route(
            "/items/:id/playback",
            get(crate::get_item_playback_position).patch(crate::update_item_playback_position),
        )
        .route("/items/:id/reindex", post(crate::reindex_item))
        .route("/items/:id/chunks", get(crate::list_item_chunks))
        .route(
            "/items/:id/understanding",
            get(video_understanding::get_item_understanding)
                .post(video_understanding::analyze_item_understanding),
        )
        .route("/chunks/:id/frame", get(crate::get_chunk_frame))
        .route(
            "/chunks/:id/video-segment",
            get(crate::get_chunk_video_segment),
        )
        .route("/chunks/:id/video-clip", get(crate::get_chunk_video_clip))
}
