use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, params_from_iter, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{sqlite, AppPaths};

static USAGE_ID_COUNTER: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UsageEvent {
    pub id: String,
    pub created_at: Option<i64>,
    pub provider_mode: String,
    pub capability: String,
    pub provider_id: Option<String>,
    pub provider_type: Option<String>,
    pub model_id: Option<String>,
    pub item_id: Option<String>,
    pub job_id: Option<String>,
    pub status: String,
    pub request_count: u64,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub audio_seconds: Option<f64>,
    pub image_count: Option<u64>,
    pub video_seconds: Option<f64>,
    pub estimated_usd: Option<f64>,
    pub billed_credits: Option<f64>,
    pub price_snapshot_id: Option<String>,
    pub metadata: Value,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct UsageTotals {
    pub event_count: u64,
    pub request_count: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub audio_seconds: f64,
    pub image_count: u64,
    pub video_seconds: f64,
    pub estimated_usd: f64,
    pub billed_credits: f64,
    pub unpriced_events: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UsageBreakdown {
    pub key: String,
    pub totals: UsageTotals,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UsageSummary {
    pub total: UsageTotals,
    pub remote: UsageTotals,
    pub local: UsageTotals,
    pub by_capability: Vec<UsageBreakdown>,
}

#[derive(Debug, Clone)]
pub struct NewUsageEvent {
    pub provider_mode: String,
    pub capability: String,
    pub provider_id: Option<String>,
    pub provider_type: Option<String>,
    pub model_id: Option<String>,
    pub item_id: Option<String>,
    pub job_id: Option<String>,
    pub status: String,
    pub request_count: u64,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub audio_seconds: Option<f64>,
    pub image_count: Option<u64>,
    pub video_seconds: Option<f64>,
    pub estimated_usd: Option<f64>,
    pub billed_credits: Option<f64>,
    pub price_snapshot_id: Option<String>,
    pub metadata: Value,
}

impl NewUsageEvent {
    pub fn new(provider_mode: impl Into<String>, capability: impl Into<String>) -> Self {
        Self {
            provider_mode: provider_mode.into(),
            capability: capability.into(),
            provider_id: None,
            provider_type: None,
            model_id: None,
            item_id: None,
            job_id: None,
            status: "succeeded".to_string(),
            request_count: 1,
            input_tokens: None,
            output_tokens: None,
            audio_seconds: None,
            image_count: None,
            video_seconds: None,
            estimated_usd: None,
            billed_credits: None,
            price_snapshot_id: None,
            metadata: Value::Object(Default::default()),
        }
    }
}

pub fn record_usage_event(paths: &AppPaths, event: NewUsageEvent) -> anyhow::Result<UsageEvent> {
    validate_mode(&event.provider_mode)?;
    validate_capability(&event.capability)?;
    validate_status(&event.status)?;
    let id = next_usage_id();
    let metadata = serde_json::to_string(&event.metadata)?;
    let conn = sqlite::open(paths)?;
    conn.execute(
        r#"
        INSERT INTO inference_usage_events (
            id, provider_mode, capability, provider_id, provider_type, model_id,
            item_id, job_id, status, request_count, input_tokens, output_tokens,
            audio_seconds, image_count, video_seconds, estimated_usd, billed_credits,
            price_snapshot_id, metadata
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19)
        "#,
        params![
            id,
            event.provider_mode,
            event.capability,
            event.provider_id,
            event.provider_type,
            event.model_id,
            event.item_id,
            event.job_id,
            event.status,
            event.request_count,
            event.input_tokens,
            event.output_tokens,
            event.audio_seconds,
            event.image_count,
            event.video_seconds,
            event.estimated_usd,
            event.billed_credits,
            event.price_snapshot_id,
            metadata,
        ],
    )?;

    get_usage_event(paths, &id)?.ok_or_else(|| anyhow::anyhow!("usage event was not recorded"))
}

pub fn get_usage_event(paths: &AppPaths, id: &str) -> anyhow::Result<Option<UsageEvent>> {
    let conn = sqlite::open(paths)?;
    conn.query_row(
        r#"
        SELECT id, created_at, provider_mode, capability, provider_id, provider_type, model_id,
               item_id, job_id, status, request_count, input_tokens, output_tokens, audio_seconds,
               image_count, video_seconds, estimated_usd, billed_credits, price_snapshot_id, metadata
        FROM inference_usage_events
        WHERE id = ?1
        "#,
        [id],
        usage_event_from_row,
    )
    .optional()
    .map_err(Into::into)
}

pub fn list_usage_events(paths: &AppPaths, limit: usize) -> anyhow::Result<Vec<UsageEvent>> {
    let conn = sqlite::open(paths)?;
    let mut stmt = conn.prepare(
        r#"
        SELECT id, created_at, provider_mode, capability, provider_id, provider_type, model_id,
               item_id, job_id, status, request_count, input_tokens, output_tokens, audio_seconds,
               image_count, video_seconds, estimated_usd, billed_credits, price_snapshot_id, metadata
        FROM inference_usage_events
        ORDER BY created_at DESC, id DESC
        LIMIT ?1
        "#,
    )?;
    let rows = stmt.query_map([limit.max(1) as i64], usage_event_from_row)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

pub fn usage_summary(paths: &AppPaths) -> anyhow::Result<UsageSummary> {
    Ok(UsageSummary {
        total: usage_totals(paths, None, None, None)?,
        remote: usage_totals_for_modes(paths, &["remote", "byok", "cloud"])?,
        local: usage_totals_for_modes(paths, &["local", "self_host"])?,
        by_capability: usage_breakdown(paths, "capability")?,
    })
}

fn usage_totals_for_modes(paths: &AppPaths, modes: &[&str]) -> anyhow::Result<UsageTotals> {
    anyhow::ensure!(!modes.is_empty(), "usage mode list cannot be empty");
    let conn = sqlite::open(paths)?;
    let placeholders = (1..=modes.len())
        .map(|index| format!("?{index}"))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        r#"
        SELECT COUNT(*), COALESCE(SUM(request_count), 0),
               COALESCE(SUM(input_tokens), 0), COALESCE(SUM(output_tokens), 0),
               COALESCE(SUM(audio_seconds), 0), COALESCE(SUM(image_count), 0),
               COALESCE(SUM(video_seconds), 0), COALESCE(SUM(estimated_usd), 0),
               COALESCE(SUM(billed_credits), 0),
               COALESCE(SUM(CASE WHEN estimated_usd IS NULL THEN 1 ELSE 0 END), 0)
        FROM inference_usage_events
        WHERE provider_mode IN ({placeholders})
        "#
    );
    conn.query_row(&sql, params_from_iter(modes.iter()), |row| {
        totals_from_row(row, 0)
    })
    .map_err(Into::into)
}

pub fn usage_totals_for_item(paths: &AppPaths, item_id: &str) -> anyhow::Result<UsageTotals> {
    usage_totals(paths, Some("item_id"), Some(item_id), None)
}

pub fn usage_totals_for_job(paths: &AppPaths, job_id: &str) -> anyhow::Result<UsageTotals> {
    usage_totals(paths, Some("job_id"), Some(job_id), None)
}

fn usage_breakdown(paths: &AppPaths, column: &str) -> anyhow::Result<Vec<UsageBreakdown>> {
    anyhow::ensure!(matches!(column, "capability" | "provider_mode"));
    let conn = sqlite::open(paths)?;
    let sql = format!(
        r#"
        SELECT {column}, COUNT(*), COALESCE(SUM(request_count), 0),
               COALESCE(SUM(input_tokens), 0), COALESCE(SUM(output_tokens), 0),
               COALESCE(SUM(audio_seconds), 0), COALESCE(SUM(image_count), 0),
               COALESCE(SUM(video_seconds), 0), COALESCE(SUM(estimated_usd), 0),
               COALESCE(SUM(billed_credits), 0),
               COALESCE(SUM(CASE WHEN estimated_usd IS NULL THEN 1 ELSE 0 END), 0)
        FROM inference_usage_events
        GROUP BY {column}
        ORDER BY {column} ASC
        "#
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], |row| {
        Ok(UsageBreakdown {
            key: row.get(0)?,
            totals: totals_from_row(row, 1)?,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

fn usage_totals(
    paths: &AppPaths,
    column: Option<&str>,
    value: Option<&str>,
    status: Option<&str>,
) -> anyhow::Result<UsageTotals> {
    if let Some(column) = column {
        anyhow::ensure!(matches!(
            column,
            "provider_mode" | "capability" | "item_id" | "job_id"
        ));
    }

    let mut sql = String::from(
        r#"
        SELECT COUNT(*), COALESCE(SUM(request_count), 0),
               COALESCE(SUM(input_tokens), 0), COALESCE(SUM(output_tokens), 0),
               COALESCE(SUM(audio_seconds), 0), COALESCE(SUM(image_count), 0),
               COALESCE(SUM(video_seconds), 0), COALESCE(SUM(estimated_usd), 0),
               COALESCE(SUM(billed_credits), 0),
               COALESCE(SUM(CASE WHEN estimated_usd IS NULL THEN 1 ELSE 0 END), 0)
        FROM inference_usage_events
        "#,
    );

    let conn = sqlite::open(paths)?;
    match (column, value, status) {
        (Some(column), Some(value), Some(status)) => {
            sql.push_str(&format!(" WHERE {column} = ?1 AND status = ?2"));
            conn.query_row(&sql, params![value, status], |row| totals_from_row(row, 0))
                .map_err(Into::into)
        }
        (Some(column), Some(value), None) => {
            sql.push_str(&format!(" WHERE {column} = ?1"));
            conn.query_row(&sql, [value], |row| totals_from_row(row, 0))
                .map_err(Into::into)
        }
        (None, None, Some(status)) => {
            sql.push_str(" WHERE status = ?1");
            conn.query_row(&sql, [status], |row| totals_from_row(row, 0))
                .map_err(Into::into)
        }
        (None, None, None) => conn
            .query_row(&sql, [], |row| totals_from_row(row, 0))
            .map_err(Into::into),
        _ => anyhow::bail!("usage totals filter is incomplete"),
    }
}

fn usage_event_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<UsageEvent> {
    let metadata: String = row.get(19)?;
    Ok(UsageEvent {
        id: row.get(0)?,
        created_at: row.get(1)?,
        provider_mode: row.get(2)?,
        capability: row.get(3)?,
        provider_id: row.get(4)?,
        provider_type: row.get(5)?,
        model_id: row.get(6)?,
        item_id: row.get(7)?,
        job_id: row.get(8)?,
        status: row.get(9)?,
        request_count: row.get::<_, i64>(10)?.max(0) as u64,
        input_tokens: optional_u64(row, 11)?,
        output_tokens: optional_u64(row, 12)?,
        audio_seconds: row.get(13)?,
        image_count: optional_u64(row, 14)?,
        video_seconds: row.get(15)?,
        estimated_usd: row.get(16)?,
        billed_credits: row.get(17)?,
        price_snapshot_id: row.get(18)?,
        metadata: serde_json::from_str(&metadata).unwrap_or(Value::Object(Default::default())),
    })
}

fn totals_from_row(row: &rusqlite::Row<'_>, offset: usize) -> rusqlite::Result<UsageTotals> {
    Ok(UsageTotals {
        event_count: row.get::<_, i64>(offset)?.max(0) as u64,
        request_count: row.get::<_, i64>(offset + 1)?.max(0) as u64,
        input_tokens: row.get::<_, i64>(offset + 2)?.max(0) as u64,
        output_tokens: row.get::<_, i64>(offset + 3)?.max(0) as u64,
        audio_seconds: row.get(offset + 4)?,
        image_count: row.get::<_, i64>(offset + 5)?.max(0) as u64,
        video_seconds: row.get(offset + 6)?,
        estimated_usd: row.get(offset + 7)?,
        billed_credits: row.get(offset + 8)?,
        unpriced_events: row.get::<_, i64>(offset + 9)?.max(0) as u64,
    })
}

fn optional_u64(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<Option<u64>> {
    row.get::<_, Option<i64>>(index)
        .map(|value| value.map(|value| value.max(0) as u64))
}

fn validate_mode(mode: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        matches!(mode, "remote" | "local"),
        "unsupported usage provider mode: {mode}"
    );
    Ok(())
}

fn validate_capability(capability: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        matches!(
            capability,
            "asr" | "embedding_text" | "embedding_image" | "video_understanding" | "search_query"
        ),
        "unsupported usage capability: {capability}"
    );
    Ok(())
}

fn validate_status(status: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        matches!(status, "succeeded" | "failed" | "estimated"),
        "unsupported usage status: {status}"
    );
    Ok(())
}

fn next_usage_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let counter = USAGE_ID_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("usage-{nanos:x}-{counter:x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_and_summarizes_usage_events() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        let mut event = NewUsageEvent::new("remote", "embedding_image");
        event.provider_id = Some("env-embedding".to_string());
        event.provider_type = Some("gemini".to_string());
        event.model_id = Some("gemini-embedding-2".to_string());
        event.image_count = Some(14);
        event.estimated_usd = Some(0.00168);
        event.metadata = serde_json::json!({ "price": "gemini-embedding-2-standard" });

        let recorded = record_usage_event(&paths, event).unwrap();
        assert_eq!(recorded.provider_mode, "remote");
        assert_eq!(recorded.capability, "embedding_image");
        assert_eq!(recorded.image_count, Some(14));

        let summary = usage_summary(&paths).unwrap();
        assert_eq!(summary.total.event_count, 1);
        assert_eq!(summary.remote.image_count, 14);
        assert!((summary.remote.estimated_usd - 0.00168).abs() < f64::EPSILON);
        assert_eq!(summary.by_capability[0].key, "embedding_image");
    }

    #[test]
    fn summary_groups_legacy_provider_modes() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();
        let conn = sqlite::open(&paths).unwrap();
        for (id, mode, usd) in [
            ("usage-remote", "remote", 1.0),
            ("usage-byok", "byok", 2.0),
            ("usage-cloud", "cloud", 3.0),
            ("usage-local", "local", 4.0),
            ("usage-self-host", "self_host", 5.0),
        ] {
            conn.execute(
                r#"
                INSERT INTO inference_usage_events (
                    id, provider_mode, capability, estimated_usd, metadata
                )
                VALUES (?1, ?2, 'asr', ?3, '{}')
                "#,
                params![id, mode, usd],
            )
            .unwrap();
        }

        let summary = usage_summary(&paths).unwrap();

        assert_eq!(summary.remote.event_count, 3);
        assert_eq!(summary.local.event_count, 2);
        assert_eq!(summary.remote.estimated_usd, 6.0);
        assert_eq!(summary.local.estimated_usd, 9.0);
    }
}
