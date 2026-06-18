use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};

use crate::paths::AppPaths;

pub const LOCAL_PROVIDER_ID: &str = "local";
pub const PROVIDER_STATUS_READY: &str = "ready";
pub const PROVIDER_STATUS_UNCONFIGURED: &str = "unconfigured";
pub const PROVIDER_STATUS_ERROR: &str = "error";

const PROVIDER_TYPE_LOCAL: &str = "local";
const PROVIDER_TYPE_OPENAI: &str = "openai";
const PROVIDER_TYPE_ANTHROPIC: &str = "anthropic";
const PROVIDER_TYPE_GEMINI: &str = "gemini";
const PROVIDER_TYPE_OPENAI_COMPATIBLE: &str = "openai-compatible";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Provider {
    pub id: String,
    #[serde(rename = "type")]
    pub provider_type: String,
    pub label: String,
    pub base_url: Option<String>,
    pub status: String,
    pub last_error: Option<String>,
    pub created_at: Option<i64>,
    pub updated_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewProvider {
    pub id: String,
    pub provider_type: String,
    pub label: String,
    pub base_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProviderUpdate {
    pub provider_type: Option<String>,
    pub label: Option<String>,
    pub base_url: Option<String>,
}

pub fn list_providers(paths: &AppPaths) -> anyhow::Result<Vec<Provider>> {
    let conn = crate::sqlite::open(paths)?;
    let mut stmt = conn.prepare(
        r#"
        SELECT id, type, label, base_url, status, last_error, created_at, updated_at
        FROM providers
        ORDER BY
            CASE id WHEN 'local' THEN 0 ELSE 1 END,
            created_at ASC,
            label ASC
        "#,
    )?;
    let rows = stmt.query_map([], provider_from_row)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

pub fn get_provider(paths: &AppPaths, id: &str) -> anyhow::Result<Option<Provider>> {
    let conn = crate::sqlite::open(paths)?;
    conn.query_row(
        r#"
        SELECT id, type, label, base_url, status, last_error, created_at, updated_at
        FROM providers
        WHERE id = ?1
        "#,
        [id],
        provider_from_row,
    )
    .optional()
    .map_err(Into::into)
}

pub fn create_provider(paths: &AppPaths, provider: NewProvider) -> anyhow::Result<Provider> {
    anyhow::ensure!(
        provider.id != LOCAL_PROVIDER_ID,
        "local provider already exists"
    );
    let provider_type = normalized_provider_type(&provider.provider_type)?;
    anyhow::ensure!(
        provider_type != PROVIDER_TYPE_LOCAL,
        "local provider is built in"
    );
    let label = normalize_label(&provider.label)?;
    let base_url = normalize_base_url(provider_type, provider.base_url.as_deref())?;
    let conn = crate::sqlite::open(paths)?;
    conn.execute(
        r#"
        INSERT INTO providers (id, type, label, base_url, status)
        VALUES (?1, ?2, ?3, ?4, 'unconfigured')
        "#,
        (&provider.id, provider_type, label, base_url),
    )?;
    get_provider(paths, &provider.id)?.ok_or_else(|| anyhow::anyhow!("provider was not created"))
}

pub fn update_provider(
    paths: &AppPaths,
    id: &str,
    changes: ProviderUpdate,
) -> anyhow::Result<Provider> {
    anyhow::ensure!(id != LOCAL_PROVIDER_ID, "local provider cannot be updated");
    let current = get_provider(paths, id)?.ok_or_else(|| anyhow::anyhow!("provider not found"))?;
    let provider_type = match changes.provider_type {
        Some(provider_type) => {
            let normalized = normalized_provider_type(&provider_type)?;
            anyhow::ensure!(
                normalized != PROVIDER_TYPE_LOCAL,
                "local provider is built in"
            );
            normalized.to_string()
        }
        None => current.provider_type.clone(),
    };
    let label = match changes.label {
        Some(label) => normalize_label(&label)?,
        None => current.label,
    };
    let base_url = match changes.base_url {
        Some(base_url) => normalize_base_url(&provider_type, Some(&base_url))?,
        None if provider_type != current.provider_type => {
            normalize_base_url(&provider_type, current.base_url.as_deref())?
        }
        None => current.base_url.clone(),
    };
    let status = if provider_type != current.provider_type || base_url != current.base_url {
        PROVIDER_STATUS_UNCONFIGURED.to_string()
    } else {
        current.status
    };
    let last_error = if status == PROVIDER_STATUS_UNCONFIGURED {
        None
    } else {
        current.last_error
    };
    let conn = crate::sqlite::open(paths)?;
    conn.execute(
        r#"
        UPDATE providers
        SET type = ?2,
            label = ?3,
            base_url = ?4,
            status = ?5,
            last_error = ?6,
            updated_at = strftime('%s','now')
        WHERE id = ?1
        "#,
        (id, provider_type, label, base_url, status, last_error),
    )?;
    get_provider(paths, id)?.ok_or_else(|| anyhow::anyhow!("provider not found"))
}

pub fn delete_provider(paths: &AppPaths, id: &str) -> anyhow::Result<()> {
    anyhow::ensure!(id != LOCAL_PROVIDER_ID, "local provider cannot be deleted");
    let conn = crate::sqlite::open(paths)?;
    let deleted = conn.execute("DELETE FROM providers WHERE id = ?1", [id])?;
    anyhow::ensure!(deleted > 0, "provider not found");
    Ok(())
}

pub fn set_provider_status(
    paths: &AppPaths,
    id: &str,
    status: &str,
    last_error: Option<&str>,
) -> anyhow::Result<Provider> {
    anyhow::ensure!(
        matches!(
            status,
            PROVIDER_STATUS_READY | PROVIDER_STATUS_UNCONFIGURED | PROVIDER_STATUS_ERROR
        ),
        "unsupported provider status: {status}"
    );
    let conn = crate::sqlite::open(paths)?;
    let updated = conn.execute(
        r#"
        UPDATE providers
        SET status = ?2,
            last_error = ?3,
            updated_at = strftime('%s','now')
        WHERE id = ?1
        "#,
        (id, status, last_error),
    )?;
    anyhow::ensure!(updated > 0, "provider not found");
    get_provider(paths, id)?.ok_or_else(|| anyhow::anyhow!("provider not found"))
}

pub fn default_base_url(provider_type: &str) -> Option<&'static str> {
    match provider_type {
        PROVIDER_TYPE_OPENAI => Some("https://api.openai.com/v1"),
        PROVIDER_TYPE_ANTHROPIC => Some("https://api.anthropic.com"),
        PROVIDER_TYPE_GEMINI => Some("https://generativelanguage.googleapis.com/v1beta"),
        _ => None,
    }
}

pub fn is_supported_remote_provider_type(provider_type: &str) -> bool {
    matches!(
        provider_type,
        PROVIDER_TYPE_OPENAI
            | PROVIDER_TYPE_ANTHROPIC
            | PROVIDER_TYPE_GEMINI
            | PROVIDER_TYPE_OPENAI_COMPATIBLE
    )
}

fn normalized_provider_type(provider_type: &str) -> anyhow::Result<&'static str> {
    match provider_type.trim() {
        PROVIDER_TYPE_LOCAL => Ok(PROVIDER_TYPE_LOCAL),
        PROVIDER_TYPE_OPENAI => Ok(PROVIDER_TYPE_OPENAI),
        PROVIDER_TYPE_ANTHROPIC => Ok(PROVIDER_TYPE_ANTHROPIC),
        PROVIDER_TYPE_GEMINI => Ok(PROVIDER_TYPE_GEMINI),
        PROVIDER_TYPE_OPENAI_COMPATIBLE => Ok(PROVIDER_TYPE_OPENAI_COMPATIBLE),
        other => anyhow::bail!("unsupported provider type: {other}"),
    }
}

fn normalize_base_url(
    provider_type: &str,
    base_url: Option<&str>,
) -> anyhow::Result<Option<String>> {
    if provider_type == PROVIDER_TYPE_LOCAL {
        return Ok(None);
    }

    let trimmed = base_url.unwrap_or_default().trim().trim_end_matches('/');
    if !trimmed.is_empty() {
        return Ok(Some(trimmed.to_string()));
    }

    if let Some(default) = default_base_url(provider_type) {
        return Ok(Some(default.to_string()));
    }

    anyhow::bail!("base_url is required for openai-compatible providers")
}

fn normalize_label(label: &str) -> anyhow::Result<String> {
    let trimmed = label.trim();
    anyhow::ensure!(!trimmed.is_empty(), "label cannot be empty");
    Ok(trimmed.to_string())
}

fn provider_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Provider> {
    Ok(Provider {
        id: row.get(0)?,
        provider_type: row.get(1)?,
        label: row.get(2)?,
        base_url: row.get(3)?,
        status: row.get(4)?,
        last_error: row.get(5)?,
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migration_seeds_local_provider() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();

        let providers = list_providers(&paths).unwrap();

        assert_eq!(providers.len(), 1);
        assert_eq!(providers[0].id, LOCAL_PROVIDER_ID);
        assert_eq!(providers[0].provider_type, "local");
        assert_eq!(providers[0].status, PROVIDER_STATUS_READY);
        assert_eq!(providers[0].base_url, None);
    }

    #[test]
    fn provider_crud_defaults_base_url_and_tracks_status() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();

        let created = create_provider(
            &paths,
            NewProvider {
                id: "provider-test".to_string(),
                provider_type: "gemini".to_string(),
                label: " Gemini ".to_string(),
                base_url: None,
            },
        )
        .unwrap();

        assert_eq!(created.label, "Gemini");
        assert_eq!(created.status, PROVIDER_STATUS_UNCONFIGURED);
        assert_eq!(
            created.base_url.as_deref(),
            Some("https://generativelanguage.googleapis.com/v1beta")
        );

        let updated = update_provider(
            &paths,
            "provider-test",
            ProviderUpdate {
                provider_type: None,
                label: Some("Gemini prod".to_string()),
                base_url: Some("https://example.com/v1beta/".to_string()),
            },
        )
        .unwrap();
        assert_eq!(updated.label, "Gemini prod");
        assert_eq!(
            updated.base_url.as_deref(),
            Some("https://example.com/v1beta")
        );

        let retargeted = update_provider(
            &paths,
            "provider-test",
            ProviderUpdate {
                provider_type: Some("openai-compatible".to_string()),
                label: None,
                base_url: Some("https://api.groq.com/openai/v1/".to_string()),
            },
        )
        .unwrap();
        assert_eq!(retargeted.provider_type, "openai-compatible");
        assert_eq!(
            retargeted.base_url.as_deref(),
            Some("https://api.groq.com/openai/v1")
        );
        assert_eq!(retargeted.status, PROVIDER_STATUS_UNCONFIGURED);
        assert_eq!(retargeted.last_error, None);

        let ready =
            set_provider_status(&paths, "provider-test", PROVIDER_STATUS_READY, None).unwrap();
        assert_eq!(ready.status, PROVIDER_STATUS_READY);
        assert_eq!(ready.last_error, None);

        delete_provider(&paths, "provider-test").unwrap();
        assert!(get_provider(&paths, "provider-test").unwrap().is_none());
    }

    #[test]
    fn local_provider_cannot_be_updated_or_deleted() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();

        assert!(update_provider(
            &paths,
            LOCAL_PROVIDER_ID,
            ProviderUpdate {
                provider_type: None,
                label: Some("Other".to_string()),
                base_url: None,
            },
        )
        .is_err());
        assert!(delete_provider(&paths, LOCAL_PROVIDER_ID).is_err());
    }

    #[test]
    fn openai_compatible_requires_base_url() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();

        let err = create_provider(
            &paths,
            NewProvider {
                id: "provider-compatible".to_string(),
                provider_type: "openai-compatible".to_string(),
                label: "Custom".to_string(),
                base_url: None,
            },
        )
        .unwrap_err()
        .to_string();

        assert!(err.contains("base_url is required"));
    }
}
