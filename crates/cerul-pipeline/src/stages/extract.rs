use std::{fs::File, io::BufReader, path::Path};

use cerul_storage::AppPaths;
use serde_json::{json, Map};

use crate::ffmpeg;

pub(crate) async fn update_item_duration_from_media(
    paths: &AppPaths,
    item_id: &str,
    media_path: &Path,
) {
    match ffmpeg::media_duration(media_path).await {
        Ok(duration_sec) => {
            if let Err(error) = cerul_storage::set_item_duration(paths, item_id, duration_sec) {
                tracing::warn!(%error, item_id, "failed to store media duration");
            }
        }
        Err(error) => {
            tracing::warn!(%error, item_id, "failed to read media duration");
        }
    }
}

pub(crate) fn read_exif_metadata(path: &Path) -> anyhow::Result<serde_json::Value> {
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);
    let exif = match exif::Reader::new().read_from_container(&mut reader) {
        Ok(exif) => exif,
        Err(error) => {
            return Ok(json!({
                "exif": {},
                "exif_error": error.to_string(),
            }));
        }
    };
    let mut fields = Map::new();

    for field in exif.fields() {
        fields.insert(
            format!("{:?}.{:?}", field.ifd_num, field.tag),
            json!(field.display_value().with_unit(&exif).to_string()),
        );
    }

    Ok(json!({ "exif": fields }))
}
