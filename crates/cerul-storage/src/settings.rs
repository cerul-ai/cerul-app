use rusqlite::OptionalExtension;

use crate::{paths::AppPaths, sqlite};

/// Read a user setting stored as a JSON-encoded value and return it as a
/// trimmed, non-empty string. Returns `None` when the key is absent, not a
/// string, or blank.
///
/// Settings are persisted by the API as `serde_json::Value::to_string()`, so a
/// string setting is stored quoted (e.g. the value column holds `"/Volumes/Media"`,
/// quotes included). This unwraps that JSON layer.
pub fn read_string_setting(paths: &AppPaths, key: &str) -> anyhow::Result<Option<String>> {
    let conn = sqlite::open(paths)?;
    let raw: Option<String> = conn
        .query_row("SELECT value FROM settings WHERE key = ?1", [key], |row| {
            row.get(0)
        })
        .optional()?;
    Ok(raw
        .and_then(|value| serde_json::from_str::<serde_json::Value>(&value).ok())
        .and_then(|value| value.as_str().map(str::to_string))
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty()))
}

#[cfg(test)]
mod tests {
    use super::read_string_setting;
    use crate::{paths::AppPaths, sqlite};

    #[test]
    fn reads_trims_and_unwraps_json_string_setting() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();

        // Absent key → None.
        assert_eq!(read_string_setting(&paths, "media_dir").unwrap(), None);

        let conn = sqlite::open(&paths).unwrap();
        conn.execute(
            "INSERT INTO settings (key, value, updated_at) VALUES ('media_dir', ?1, 0)",
            ["\"  /Volumes/Media  \""],
        )
        .unwrap();
        assert_eq!(
            read_string_setting(&paths, "media_dir").unwrap(),
            Some("/Volumes/Media".to_string())
        );

        // Blank string → None (treated as "unset / use default").
        conn.execute(
            "UPDATE settings SET value = '\"\"' WHERE key = 'media_dir'",
            [],
        )
        .unwrap();
        assert_eq!(read_string_setting(&paths, "media_dir").unwrap(), None);
    }
}
