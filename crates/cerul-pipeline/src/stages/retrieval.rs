use cerul_storage::AppPaths;

pub(crate) fn set_embedding_index_status(
    paths: &AppPaths,
    item_id: &str,
    status: &str,
    error: Option<&str>,
    text_vectors: usize,
    image_vectors: usize,
) -> anyhow::Result<()> {
    cerul_storage::update_item_metadata(paths, item_id, |metadata| {
        metadata.insert(
            "embedding_index_status".to_string(),
            serde_json::Value::String(status.to_string()),
        );
        metadata.insert(
            "embedding_text_vectors".to_string(),
            serde_json::Value::from(text_vectors as u64),
        );
        metadata.insert(
            "embedding_image_vectors".to_string(),
            serde_json::Value::from(image_vectors as u64),
        );
        match error {
            Some(error) => {
                metadata.insert(
                    "embedding_index_error".to_string(),
                    serde_json::Value::String(error.to_string()),
                );
            }
            None => {
                metadata.remove("embedding_index_error");
            }
        }
    })
}
