pub mod chunks;
pub mod items;
pub mod logs;
pub mod paths;
pub mod providers;
pub mod retrieval;
pub mod sqlite;
pub mod usage;
pub mod vectors;

pub use chunks::{
    replace_item_keyframes, replace_media_embeddings, replace_media_embeddings_for_profile,
    replace_media_embeddings_with_ocr, replace_media_embeddings_with_ocr_for_profile,
    replace_video_embeddings_with_ocr, replace_video_embeddings_with_ocr_for_profile,
    write_media_chunks, write_media_chunks_with_ocr_and_lines, write_media_sqlite_chunks,
    write_media_sqlite_chunks_with_ocr, write_media_sqlite_chunks_with_ocr_and_lines,
    write_video_chunks, write_video_chunks_with_ocr, write_video_sqlite_chunks_with_ocr,
    StorageImageChunk, StorageOcrChunk, StorageTranscriptChunk, StorageTranscriptLine,
    StorageWriteSummary,
};
pub use items::{
    get_item, item_ids_for_source, mark_indexed, set_item_duration, set_item_raw_path,
    set_video_index_status, set_video_multimodal_index_status, update_item_metadata, StoredItem,
};
pub use logs::{append_jsonl_event, log_file_path};
pub use paths::AppPaths;
pub use retrieval::{
    best_sub_unit_for_query, best_visual_sub_unit_for_query, clear_item_search_index,
    indexed_item_count, item_has_retrieval_units, item_retrieval_unit_count,
    items_needing_rebuild_count, rebuild_item_retrieval_units, replace_item_retrieval_units,
    retrieval_unit_count, set_item_search_index_status, StorageRetrievalUnit, SEARCH_INDEX_VERSION,
};
pub use usage::{
    list_usage_events, record_usage_event, usage_summary, usage_totals_by_item,
    usage_totals_by_item_ids, usage_totals_by_job, usage_totals_by_job_ids, usage_totals_for_item,
    usage_totals_for_job, NewUsageEvent, UsageBreakdown, UsageEvent, UsageSummary, UsageTotals,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageBackend {
    Sqlite,
    Qdrant,
}

pub fn crate_ready() -> bool {
    true
}

pub fn required_backends() -> &'static [StorageBackend] {
    &[StorageBackend::Sqlite, StorageBackend::Qdrant]
}

#[cfg(test)]
mod tests {
    use super::{required_backends, StorageBackend};

    #[test]
    fn storage_scaffold_exposes_required_backends() {
        assert_eq!(
            required_backends(),
            &[StorageBackend::Sqlite, StorageBackend::Qdrant]
        );
        assert!(!rusqlite::version().is_empty());
    }
}
