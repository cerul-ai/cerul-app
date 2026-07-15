use std::{path::PathBuf, time::Instant};

use cerul_storage::{
    vectors::EmbeddingProfile, AppPaths, StorageRetrievalUnit, StorageWriteSummary,
};

pub(crate) struct RetrievalEmbeddingPlan {
    units: Vec<StorageRetrievalUnit>,
    text_unit_indexes: Vec<usize>,
    image_unit_indexes: Vec<usize>,
    text_inputs: Vec<String>,
    image_paths: Vec<PathBuf>,
}

impl RetrievalEmbeddingPlan {
    pub(crate) fn unit_count(&self) -> usize {
        self.units.len()
    }

    pub(crate) fn text_unit_count(&self) -> usize {
        self.text_unit_indexes.len()
    }

    pub(crate) fn image_unit_count(&self) -> usize {
        self.image_unit_indexes.len()
    }

    pub(crate) fn text_inputs(&self) -> &[String] {
        &self.text_inputs
    }

    pub(crate) fn image_paths(&self) -> &[PathBuf] {
        &self.image_paths
    }

    pub(crate) fn units(&self) -> &[StorageRetrievalUnit] {
        &self.units
    }
}

pub(crate) struct RetrievalIndexWriteSummary {
    pub(crate) write_summary: StorageWriteSummary,
    pub(crate) vector_count: usize,
    pub(crate) vector_index_write_ms: u64,
    pub(crate) stale_vectors_deleted: usize,
}

pub(crate) fn build_retrieval_embedding_plan(
    paths: &AppPaths,
    item_id: &str,
    profile_id: &str,
    include_image_embeddings: bool,
) -> anyhow::Result<RetrievalEmbeddingPlan> {
    let units = cerul_storage::build_item_retrieval_units(paths, item_id, profile_id)?;
    anyhow::ensure!(
        !units.is_empty(),
        "no retrieval units generated for item {item_id}"
    );
    Ok(plan_from_units(units, include_image_embeddings))
}

fn plan_from_units(
    units: Vec<StorageRetrievalUnit>,
    include_image_embeddings: bool,
) -> RetrievalEmbeddingPlan {
    let text_unit_indexes = units
        .iter()
        .enumerate()
        .filter_map(|(index, unit)| (!unit.uses_image_embedding()).then_some(index))
        .collect::<Vec<_>>();
    let image_unit_indexes = if include_image_embeddings {
        units
            .iter()
            .enumerate()
            .filter_map(|(index, unit)| unit.has_image_embedding_source().then_some(index))
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    let text_inputs = text_unit_indexes
        .iter()
        .map(|&index| units[index].content_text.clone())
        .collect::<Vec<_>>();
    let image_paths = image_unit_indexes
        .iter()
        .filter_map(|&index| {
            units[index]
                .representative_frame_path
                .as_ref()
                .map(PathBuf::from)
        })
        .collect::<Vec<_>>();

    RetrievalEmbeddingPlan {
        units,
        text_unit_indexes,
        image_unit_indexes,
        text_inputs,
        image_paths,
    }
}

pub(crate) async fn write_unified_retrieval_vectors(
    paths: &AppPaths,
    item_id: &str,
    plan: &RetrievalEmbeddingPlan,
    text_vectors: &[Vec<f32>],
    image_vectors: &[Vec<f32>],
    profile: &EmbeddingProfile,
    replace_existing_vectors: bool,
) -> anyhow::Result<RetrievalIndexWriteSummary> {
    anyhow::ensure!(
        text_vectors.len() == plan.text_unit_indexes.len(),
        "retrieval text unit count ({}) does not match vector count ({})",
        plan.text_unit_indexes.len(),
        text_vectors.len()
    );
    if !image_vectors.is_empty() {
        anyhow::ensure!(
            image_vectors.len() == plan.image_unit_indexes.len(),
            "retrieval image unit count ({}) does not match vector count ({})",
            plan.image_unit_indexes.len(),
            image_vectors.len()
        );
    }

    let mut records = Vec::with_capacity(text_vectors.len() + image_vectors.len());
    for (&unit_index, vector) in plan.text_unit_indexes.iter().zip(text_vectors.iter()) {
        let unit = &plan.units[unit_index];
        records.push(cerul_storage::vectors::VectorRecord::new_for_dimensions(
            unit.id.clone(),
            unit.item_id.clone(),
            vector.clone(),
            profile.output_dimension,
        )?);
    }
    for (&unit_index, vector) in plan.image_unit_indexes.iter().zip(image_vectors.iter()) {
        let unit = &plan.units[unit_index];
        records.push(
            cerul_storage::vectors::VectorRecord::new_for_dimensions_with_point_key(
                format!("{}:image", unit.id),
                unit.id.clone(),
                unit.item_id.clone(),
                vector.clone(),
                profile.output_dimension,
            )?,
        );
    }

    let vector_index_started = Instant::now();
    let stale_vectors_deleted = if replace_existing_vectors {
        cerul_storage::vectors::replace_item_unified_embeddings_for_profile(
            paths,
            item_id,
            &records,
            profile,
            cerul_storage::SEARCH_INDEX_VERSION,
        )
        .await?;
        0
    } else {
        cerul_storage::vectors::upsert_item_unified_embeddings_for_profile(
            paths,
            &records,
            profile,
            cerul_storage::SEARCH_INDEX_VERSION,
        )
        .await?;
        cerul_storage::vectors::delete_stale_item_unified_embeddings_for_profile(
            paths,
            item_id,
            &records,
            profile,
            cerul_storage::SEARCH_INDEX_VERSION,
        )
        .await?
    };
    let vector_index_write_ms = vector_index_started.elapsed().as_millis() as u64;
    Ok(RetrievalIndexWriteSummary {
        write_summary: StorageWriteSummary {
            transcript_chunks: plan.units.len(),
            keyframes: plan.image_paths.len(),
            text_vectors: text_vectors.len(),
            image_vectors: image_vectors.len(),
        },
        vector_count: records.len(),
        vector_index_write_ms,
        stale_vectors_deleted,
    })
}

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

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn retrieval_unit(
        id: &str,
        unit_kind: &str,
        text: &str,
        frame_path: Option<&str>,
        transcript_text: Option<&str>,
    ) -> StorageRetrievalUnit {
        StorageRetrievalUnit {
            id: id.to_string(),
            item_id: "item-1".to_string(),
            unit_index: 0,
            unit_kind: unit_kind.to_string(),
            start_sec: None,
            end_sec: None,
            content_text: text.to_string(),
            transcript_text: transcript_text.map(str::to_string),
            ocr_text: None,
            visual_text: None,
            summary_text: None,
            representative_chunk_id: None,
            representative_frame_path: frame_path.map(str::to_string),
            embedding_profile_id: "profile-1".to_string(),
            index_version: 2,
            metadata: json!({}),
        }
    }

    #[test]
    fn plan_from_units_separates_text_inputs_and_image_paths() {
        let plan = plan_from_units(
            vec![
                retrieval_unit("text-1", "transcript", "hello", None, Some("hello")),
                retrieval_unit("image-1", "image", "visual", Some("/tmp/frame.jpg"), None),
            ],
            true,
        );

        assert_eq!(plan.unit_count(), 2);
        assert_eq!(plan.text_unit_count(), 1);
        assert_eq!(plan.image_unit_count(), 1);
        assert_eq!(plan.text_inputs(), &["hello".to_string()]);
        assert_eq!(plan.image_paths(), &[PathBuf::from("/tmp/frame.jpg")]);
    }
}
