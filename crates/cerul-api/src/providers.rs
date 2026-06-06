use std::{collections::BTreeMap, env, fs, time::Duration};

use axum::{
    extract::{Path, State},
    Json,
};
use cerul_storage::AppPaths;
use keyring::Entry;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::{new_id, ApiError, ApiResult, ApiState};

const KEYCHAIN_SERVICE: &str = "ai.cerul.providers";
const TEST_TIMEOUT: Duration = Duration::from_secs(10);
const OFFICIAL_OPENAI_BASE_URL: &str = "https://api.openai.com/v1";
const ENV_ASR_PROVIDER_ID: &str = "env-asr";
const ENV_EMBEDDING_PROVIDER_ID: &str = "env-embedding";
const ENV_VIDEO_UNDERSTANDING_PROVIDER_ID: &str = "env-video-understanding";

#[derive(Debug, Clone, Serialize)]
pub struct ProviderRecord {
    pub id: String,
    #[serde(rename = "type")]
    pub provider_type: String,
    pub label: String,
    pub base_url: Option<String>,
    pub status: String,
    pub last_error: Option<String>,
    pub has_key: bool,
    pub created_at: Option<i64>,
    pub updated_at: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct CreateProviderRequest {
    #[serde(rename = "type")]
    pub provider_type: String,
    pub label: String,
    pub base_url: Option<String>,
    pub api_key: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub struct UpdateProviderRequest {
    pub label: Option<String>,
    pub base_url: Option<String>,
    pub api_key: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProviderTestRequest {
    url: String,
    headers: Vec<(String, String)>,
}

pub async fn list_providers(State(state): State<ApiState>) -> ApiResult<Json<Vec<ProviderRecord>>> {
    let providers = cerul_storage::providers::list_providers(&state.paths)?;
    Ok(Json(
        providers
            .into_iter()
            .map(|provider| provider_record(provider, &state.paths))
            .collect(),
    ))
}

pub(crate) fn bootstrap_env_providers(paths: &AppPaths) -> anyhow::Result<()> {
    if env_setting("CERUL_ASR_API_KEY").is_some() {
        let asr_base_url = env_base_url("CERUL_ASR_BASE_URL");
        let asr_model = env_setting("CERUL_ASR_MODEL")
            .unwrap_or_else(|| crate::models::DEFAULT_ASR_MODEL_ID.to_string());
        let asr_provider_type = infer_asr_provider_type(&asr_model, asr_base_url.as_deref());
        ensure_env_provider(
            paths,
            ENV_ASR_PROVIDER_ID,
            asr_provider_type,
            "ASR (.env)",
            asr_base_url,
        )?;
        select_provider_if_missing_key(paths, "asr_provider_id", ENV_ASR_PROVIDER_ID)?;
    }

    if env_setting("CERUL_EMBEDDING_API_KEY").is_some() {
        ensure_env_provider(
            paths,
            ENV_EMBEDDING_PROVIDER_ID,
            "gemini",
            "Embedding (.env)",
            env_base_url("CERUL_EMBEDDING_BASE_URL"),
        )?;
        select_provider_if_missing_key(paths, "embedding_provider_id", ENV_EMBEDDING_PROVIDER_ID)?;
    }

    if env_setting("CERUL_VIDEO_UNDERSTANDING_API_KEY").is_some() {
        ensure_env_provider(
            paths,
            ENV_VIDEO_UNDERSTANDING_PROVIDER_ID,
            "gemini",
            "Video understanding (.env)",
            env_base_url("CERUL_VIDEO_UNDERSTANDING_BASE_URL"),
        )?;
        select_provider_if_missing_key(
            paths,
            "video_understanding_provider_id",
            ENV_VIDEO_UNDERSTANDING_PROVIDER_ID,
        )?;
    }

    Ok(())
}

pub async fn create_provider(
    State(state): State<ApiState>,
    Json(req): Json<CreateProviderRequest>,
) -> ApiResult<Json<ProviderRecord>> {
    if !cerul_storage::providers::is_supported_remote_provider_type(&req.provider_type) {
        return Err(ApiError::bad_request("unsupported provider type"));
    }
    validate_label(&req.label)?;
    if req.provider_type == "openai-compatible" && is_blank(req.base_url.as_deref()) {
        return Err(ApiError::bad_request(
            "base_url is required for openai-compatible providers",
        ));
    }

    let id = new_id("provider");
    let created = cerul_storage::providers::create_provider(
        &state.paths,
        cerul_storage::providers::NewProvider {
            id: id.clone(),
            provider_type: req.provider_type,
            label: req.label,
            base_url: req.base_url,
        },
    )?;

    if let Some(api_key) = clean_api_key(req.api_key) {
        if let Err(error) = set_provider_key(&state.paths, &id, &api_key) {
            let _ = cerul_storage::providers::delete_provider(&state.paths, &id);
            return Err(error.into());
        }
    }

    Ok(Json(provider_record(created, &state.paths)))
}

pub async fn update_provider(
    State(state): State<ApiState>,
    Path(id): Path<String>,
    Json(req): Json<UpdateProviderRequest>,
) -> ApiResult<Json<ProviderRecord>> {
    if id == cerul_storage::providers::LOCAL_PROVIDER_ID {
        return Err(ApiError::bad_request("local provider cannot be updated"));
    }
    let existing = cerul_storage::providers::get_provider(&state.paths, &id)?
        .ok_or_else(|| ApiError::not_found("provider not found"))?;
    if let Some(label) = req.label.as_deref() {
        validate_label(label)?;
    }
    if existing.provider_type == "openai-compatible"
        && req
            .base_url
            .as_deref()
            .is_some_and(|base_url| base_url.trim().is_empty())
    {
        return Err(ApiError::bad_request(
            "base_url is required for openai-compatible providers",
        ));
    }

    if let Some(api_key) = clean_api_key(req.api_key) {
        set_provider_key(&state.paths, &id, &api_key)?;
    }

    let updated = cerul_storage::providers::update_provider(
        &state.paths,
        &id,
        cerul_storage::providers::ProviderUpdate {
            label: req.label,
            base_url: req.base_url,
        },
    )?;
    Ok(Json(provider_record(updated, &state.paths)))
}

pub async fn delete_provider(
    State(state): State<ApiState>,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    if id == cerul_storage::providers::LOCAL_PROVIDER_ID {
        return Err(ApiError::bad_request("local provider cannot be deleted"));
    }
    let _existing = cerul_storage::providers::get_provider(&state.paths, &id)?
        .ok_or_else(|| ApiError::not_found("provider not found"))?;
    cerul_storage::providers::delete_provider(&state.paths, &id)?;
    if let Err(error) = delete_provider_key(&state.paths, &id) {
        tracing::warn!(
            %error,
            provider_id = %id,
            "provider row deleted but keychain cleanup failed"
        );
    }
    Ok(Json(json!({ "status": "deleted", "id": id })))
}

pub async fn test_provider(
    State(state): State<ApiState>,
    Path(id): Path<String>,
) -> ApiResult<Json<ProviderRecord>> {
    let provider = cerul_storage::providers::get_provider(&state.paths, &id)?
        .ok_or_else(|| ApiError::not_found("provider not found"))?;

    if provider.id == cerul_storage::providers::LOCAL_PROVIDER_ID {
        let ready = cerul_storage::providers::set_provider_status(
            &state.paths,
            &id,
            cerul_storage::providers::PROVIDER_STATUS_READY,
            None,
        )?;
        return Ok(Json(provider_record(ready, &state.paths)));
    }

    let outcome = test_remote_provider(&state.paths, &provider).await;
    let updated = match outcome {
        Ok(()) => cerul_storage::providers::set_provider_status(
            &state.paths,
            &id,
            cerul_storage::providers::PROVIDER_STATUS_READY,
            None,
        )?,
        Err(error) => {
            let message = error.to_string();
            cerul_storage::providers::set_provider_status(
                &state.paths,
                &id,
                cerul_storage::providers::PROVIDER_STATUS_ERROR,
                Some(&message),
            )?
        }
    };

    Ok(Json(provider_record(updated, &state.paths)))
}

async fn test_remote_provider(
    paths: &AppPaths,
    provider: &cerul_storage::providers::Provider,
) -> anyhow::Result<()> {
    let key = get_provider_key_for_provider(paths, provider)?
        .ok_or_else(|| anyhow::anyhow!("API key is not configured"))?;
    run_provider_test(provider, &key)
        .await
        .map_err(|error| anyhow::anyhow!(redact_secret(&error.to_string(), &key)))
}

async fn run_provider_test(
    provider: &cerul_storage::providers::Provider,
    key: &str,
) -> anyhow::Result<()> {
    let spec = provider_test_request(provider, &key)?;
    let client = reqwest::Client::builder().timeout(TEST_TIMEOUT).build()?;
    let mut request = client.get(spec.url);
    for (name, value) in spec.headers {
        request = request.header(name, value);
    }
    let response = request.send().await?;
    anyhow::ensure!(
        response.status().is_success(),
        "provider returned HTTP {}",
        response.status()
    );
    Ok(())
}

fn validate_label(label: &str) -> ApiResult<()> {
    if label.trim().is_empty() {
        return Err(ApiError::bad_request("label cannot be empty"));
    }
    Ok(())
}

fn is_blank(value: Option<&str>) -> bool {
    value.is_none_or(|item| item.trim().is_empty())
}

fn provider_test_request(
    provider: &cerul_storage::providers::Provider,
    api_key: &str,
) -> anyhow::Result<ProviderTestRequest> {
    let base_url = provider
        .base_url
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("base_url is not configured"))?
        .trim_end_matches('/');

    match provider.provider_type.as_str() {
        "openai" | "openai-compatible" => Ok(ProviderTestRequest {
            url: format!("{base_url}/models"),
            headers: vec![(
                "Authorization".to_string(),
                format!("Bearer {}", api_key.trim()),
            )],
        }),
        "gemini" => Ok(ProviderTestRequest {
            url: format!("{base_url}/models?key={}", api_key.trim()),
            headers: Vec::new(),
        }),
        "anthropic" => Ok(ProviderTestRequest {
            url: format!("{base_url}/v1/models"),
            headers: vec![
                ("x-api-key".to_string(), api_key.trim().to_string()),
                ("anthropic-version".to_string(), "2023-06-01".to_string()),
            ],
        }),
        other => anyhow::bail!("provider type {other} cannot be tested through HTTP"),
    }
}

fn provider_record(
    provider: cerul_storage::providers::Provider,
    paths: &AppPaths,
) -> ProviderRecord {
    let has_key = provider.id != cerul_storage::providers::LOCAL_PROVIDER_ID
        && has_provider_key_for_provider(paths, &provider);
    let status = if has_key && is_env_provider_id(&provider.id) && provider.status == "unconfigured"
    {
        cerul_storage::providers::PROVIDER_STATUS_READY.to_string()
    } else {
        provider.status
    };
    ProviderRecord {
        has_key,
        id: provider.id,
        provider_type: provider.provider_type,
        label: provider.label,
        base_url: provider.base_url,
        status,
        last_error: provider.last_error,
        created_at: provider.created_at,
        updated_at: provider.updated_at,
    }
}

fn is_env_provider_id(provider_id: &str) -> bool {
    matches!(
        provider_id,
        ENV_ASR_PROVIDER_ID | ENV_EMBEDDING_PROVIDER_ID | ENV_VIDEO_UNDERSTANDING_PROVIDER_ID
    )
}

fn ensure_env_provider(
    paths: &AppPaths,
    id: &str,
    provider_type: &str,
    label: &str,
    base_url: Option<String>,
) -> anyhow::Result<()> {
    if let Some(existing) = cerul_storage::providers::get_provider(paths, id)? {
        if existing.provider_type != provider_type {
            cerul_storage::providers::delete_provider(paths, id)?;
        } else {
            cerul_storage::providers::update_provider(
                paths,
                id,
                cerul_storage::providers::ProviderUpdate {
                    label: Some(label.to_string()),
                    base_url,
                },
            )?;
            return Ok(());
        }
    }

    cerul_storage::providers::create_provider(
        paths,
        cerul_storage::providers::NewProvider {
            id: id.to_string(),
            provider_type: provider_type.to_string(),
            label: label.to_string(),
            base_url,
        },
    )?;
    Ok(())
}

fn select_provider_if_missing_key(
    paths: &AppPaths,
    setting_key: &str,
    provider_id: &str,
) -> anyhow::Result<()> {
    if selected_provider_has_key(paths, setting_key)? {
        return Ok(());
    }
    let conn = cerul_storage::sqlite::open(paths)?;
    conn.execute(
        r#"
        INSERT INTO settings (key, value, updated_at)
        VALUES (?1, ?2, strftime('%s','now'))
        ON CONFLICT(key) DO UPDATE SET
            value = excluded.value,
            updated_at = excluded.updated_at
        "#,
        (
            setting_key,
            Value::String(provider_id.to_string()).to_string(),
        ),
    )?;
    Ok(())
}

fn selected_provider_has_key(paths: &AppPaths, setting_key: &str) -> anyhow::Result<bool> {
    let Some(provider_id) = crate::setting_string(paths, setting_key)? else {
        return Ok(false);
    };
    let Some(provider) = cerul_storage::providers::get_provider(paths, &provider_id)? else {
        return Ok(false);
    };
    Ok(has_provider_key_for_provider(paths, &provider))
}

fn clean_api_key(api_key: Option<String>) -> Option<String> {
    api_key
        .map(|key| key.trim().to_string())
        .filter(|key| !key.is_empty())
}

fn redact_secret(message: &str, secret: &str) -> String {
    let secret = secret.trim();
    if secret.is_empty() {
        return message.to_string();
    }
    message.replace(secret, "[redacted]")
}

fn set_provider_key(paths: &AppPaths, provider_id: &str, api_key: &str) -> anyhow::Result<()> {
    match provider_key_entry(provider_id).and_then(|entry| {
        entry.set_password(api_key)?;
        provider_key_entry(provider_id)
    }) {
        Ok(entry) => match entry.get_password() {
            Ok(value) if value == api_key => {
                let _ = delete_provider_key_fallback(paths, provider_id);
                return Ok(());
            }
            Ok(_) | Err(_) => {
                tracing::warn!(
                    provider_id = %provider_id,
                    "provider keychain write could not be verified; using local fallback"
                );
            }
        },
        Err(error) => {
            tracing::warn!(
                %error,
                provider_id = %provider_id,
                "provider keychain write failed; using local fallback"
            );
        }
    }

    set_provider_key_fallback(paths, provider_id, api_key)?;
    Ok(())
}

pub(crate) fn get_provider_key(
    paths: &AppPaths,
    provider_id: &str,
) -> anyhow::Result<Option<String>> {
    match provider_key_entry(provider_id) {
        Ok(entry) => match entry.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => get_provider_key_fallback(paths, provider_id),
            Err(error) => {
                tracing::warn!(
                    %error,
                    provider_id = %provider_id,
                    "provider keychain read failed; using local fallback"
                );
                get_provider_key_fallback(paths, provider_id)
            }
        },
        Err(error) => {
            tracing::warn!(
                %error,
                provider_id = %provider_id,
                "provider keychain entry unavailable; using local fallback"
            );
            get_provider_key_fallback(paths, provider_id)
        }
    }
}

pub(crate) fn get_provider_key_for_provider(
    paths: &AppPaths,
    provider: &cerul_storage::providers::Provider,
) -> anyhow::Result<Option<String>> {
    Ok(get_provider_key(paths, &provider.id)?
        .filter(|key| !key.trim().is_empty())
        .or_else(|| provider_env_key(provider)))
}

pub(crate) fn has_provider_key_for_provider(
    paths: &AppPaths,
    provider: &cerul_storage::providers::Provider,
) -> bool {
    get_provider_key_for_provider(paths, provider)
        .ok()
        .flatten()
        .is_some()
}

fn delete_provider_key(paths: &AppPaths, provider_id: &str) -> anyhow::Result<()> {
    match provider_key_entry(provider_id) {
        Ok(entry) => match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => {}
            Err(error) => tracing::warn!(
                %error,
                provider_id = %provider_id,
                "provider keychain delete failed"
            ),
        },
        Err(error) => tracing::warn!(
            %error,
            provider_id = %provider_id,
            "provider keychain entry unavailable during delete"
        ),
    }
    delete_provider_key_fallback(paths, provider_id)
}

fn provider_key_entry(provider_id: &str) -> anyhow::Result<Entry> {
    Ok(Entry::new(KEYCHAIN_SERVICE, provider_id)?)
}

fn provider_env_key(provider: &cerul_storage::providers::Provider) -> Option<String> {
    match provider.id.as_str() {
        ENV_ASR_PROVIDER_ID => env_setting("CERUL_ASR_API_KEY"),
        ENV_EMBEDDING_PROVIDER_ID => env_setting("CERUL_EMBEDDING_API_KEY"),
        ENV_VIDEO_UNDERSTANDING_PROVIDER_ID => env_setting("CERUL_VIDEO_UNDERSTANDING_API_KEY"),
        _ => None,
    }
}

fn env_setting(name: &str) -> Option<String> {
    env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn env_base_url(name: &str) -> Option<String> {
    env_setting(name)
        .map(|url| url.trim().trim_end_matches('/').to_string())
        .filter(|url| !url.is_empty())
}

fn infer_asr_provider_type(model: &str, base_url: Option<&str>) -> &'static str {
    if model.trim_start_matches("models/").starts_with("gemini-") {
        return "gemini";
    }
    if base_url.is_some_and(|base_url| !is_official_openai_base_url(base_url)) {
        return "openai-compatible";
    }
    "openai"
}

fn is_official_openai_base_url(base_url: &str) -> bool {
    base_url.trim().trim_end_matches('/') == OFFICIAL_OPENAI_BASE_URL
}

fn provider_key_fallback_path(paths: &AppPaths) -> std::path::PathBuf {
    paths.data.join("provider-keys.json")
}

fn read_provider_key_fallbacks(paths: &AppPaths) -> anyhow::Result<BTreeMap<String, String>> {
    let path = provider_key_fallback_path(paths);
    if !path.exists() {
        return Ok(BTreeMap::new());
    }
    let raw = fs::read_to_string(path)?;
    Ok(serde_json::from_str(&raw)?)
}

fn write_provider_key_fallbacks(
    paths: &AppPaths,
    keys: &BTreeMap<String, String>,
) -> anyhow::Result<()> {
    let path = provider_key_fallback_path(paths);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&path, serde_json::to_vec_pretty(keys)?)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

fn set_provider_key_fallback(
    paths: &AppPaths,
    provider_id: &str,
    api_key: &str,
) -> anyhow::Result<()> {
    let mut keys = read_provider_key_fallbacks(paths)?;
    keys.insert(provider_id.to_string(), api_key.to_string());
    write_provider_key_fallbacks(paths, &keys)
}

fn get_provider_key_fallback(
    paths: &AppPaths,
    provider_id: &str,
) -> anyhow::Result<Option<String>> {
    Ok(read_provider_key_fallbacks(paths)?.remove(provider_id))
}

fn delete_provider_key_fallback(paths: &AppPaths, provider_id: &str) -> anyhow::Result<()> {
    let mut keys = read_provider_key_fallbacks(paths)?;
    if keys.remove(provider_id).is_some() {
        write_provider_key_fallbacks(paths, &keys)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, MutexGuard};

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    struct EnvGuard {
        _lock: MutexGuard<'static, ()>,
    }

    impl EnvGuard {
        fn new() -> Self {
            let lock = ENV_LOCK.lock().unwrap();
            for key in [
                "CERUL_ASR_MODEL",
                "CERUL_ASR_API_KEY",
                "CERUL_ASR_BASE_URL",
                "CERUL_EMBEDDING_MODEL",
                "CERUL_EMBEDDING_API_KEY",
                "CERUL_EMBEDDING_BASE_URL",
                "CERUL_VIDEO_UNDERSTANDING_MODEL",
                "CERUL_VIDEO_UNDERSTANDING_API_KEY",
                "CERUL_VIDEO_UNDERSTANDING_BASE_URL",
            ] {
                std::env::remove_var(key);
            }
            Self { _lock: lock }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            for key in [
                "CERUL_ASR_MODEL",
                "CERUL_ASR_API_KEY",
                "CERUL_ASR_BASE_URL",
                "CERUL_EMBEDDING_MODEL",
                "CERUL_EMBEDDING_API_KEY",
                "CERUL_EMBEDDING_BASE_URL",
                "CERUL_VIDEO_UNDERSTANDING_MODEL",
                "CERUL_VIDEO_UNDERSTANDING_API_KEY",
                "CERUL_VIDEO_UNDERSTANDING_BASE_URL",
            ] {
                std::env::remove_var(key);
            }
        }
    }

    fn provider(provider_type: &str, base_url: &str) -> cerul_storage::providers::Provider {
        cerul_storage::providers::Provider {
            id: format!("provider-{provider_type}"),
            provider_type: provider_type.to_string(),
            label: provider_type.to_string(),
            base_url: Some(base_url.to_string()),
            status: "unconfigured".to_string(),
            last_error: None,
            created_at: None,
            updated_at: None,
        }
    }

    #[test]
    fn provider_test_request_matches_provider_type() {
        let openai =
            provider_test_request(&provider("openai", "https://api.openai.com/v1/"), "k").unwrap();
        assert_eq!(openai.url, "https://api.openai.com/v1/models");
        assert_eq!(
            openai.headers,
            vec![("Authorization".to_string(), "Bearer k".to_string())]
        );

        let gemini = provider_test_request(
            &provider("gemini", "https://generativelanguage.googleapis.com/v1beta"),
            "g",
        )
        .unwrap();
        assert_eq!(
            gemini.url,
            "https://generativelanguage.googleapis.com/v1beta/models?key=g"
        );
        assert!(gemini.headers.is_empty());

        let anthropic =
            provider_test_request(&provider("anthropic", "https://api.anthropic.com"), "a")
                .unwrap();
        assert_eq!(anthropic.url, "https://api.anthropic.com/v1/models");
        assert!(anthropic
            .headers
            .contains(&("anthropic-version".to_string(), "2023-06-01".to_string())));
    }

    #[test]
    fn env_bootstrap_uses_official_asr_by_default() {
        let _env = EnvGuard::new();
        std::env::set_var("CERUL_ASR_API_KEY", "test-key");
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();

        bootstrap_env_providers(&paths).unwrap();

        let provider = cerul_storage::providers::get_provider(&paths, ENV_ASR_PROVIDER_ID)
            .unwrap()
            .unwrap();
        assert_eq!(provider.provider_type, "openai");
        assert_eq!(provider.base_url.as_deref(), Some(OFFICIAL_OPENAI_BASE_URL));
    }

    #[test]
    fn env_bootstrap_treats_custom_asr_url_as_openai_compatible() {
        let _env = EnvGuard::new();
        std::env::set_var("CERUL_ASR_API_KEY", "test-key");
        std::env::set_var("CERUL_ASR_BASE_URL", "https://gateway.example/v1/");
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();

        bootstrap_env_providers(&paths).unwrap();

        let provider = cerul_storage::providers::get_provider(&paths, ENV_ASR_PROVIDER_ID)
            .unwrap()
            .unwrap();
        assert_eq!(provider.provider_type, "openai-compatible");
        assert_eq!(
            provider.base_url.as_deref(),
            Some("https://gateway.example/v1")
        );
    }

    #[test]
    fn env_bootstrap_uses_gemini_for_gemini_asr_model() {
        let _env = EnvGuard::new();
        std::env::set_var("CERUL_ASR_MODEL", "gemini-2.5-flash");
        std::env::set_var("CERUL_ASR_API_KEY", "test-key");
        std::env::set_var("CERUL_ASR_BASE_URL", "https://gemini.example/v1beta/");
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();

        bootstrap_env_providers(&paths).unwrap();

        let provider = cerul_storage::providers::get_provider(&paths, ENV_ASR_PROVIDER_ID)
            .unwrap()
            .unwrap();
        assert_eq!(provider.provider_type, "gemini");
        assert_eq!(
            provider.base_url.as_deref(),
            Some("https://gemini.example/v1beta")
        );
    }

    #[test]
    fn env_bootstrap_applies_custom_embedding_base_url() {
        let _env = EnvGuard::new();
        std::env::set_var("CERUL_EMBEDDING_API_KEY", "test-key");
        std::env::set_var("CERUL_EMBEDDING_BASE_URL", "https://gemini.example/v1beta/");
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();

        bootstrap_env_providers(&paths).unwrap();

        let provider = cerul_storage::providers::get_provider(&paths, ENV_EMBEDDING_PROVIDER_ID)
            .unwrap()
            .unwrap();
        assert_eq!(provider.provider_type, "gemini");
        assert_eq!(
            provider.base_url.as_deref(),
            Some("https://gemini.example/v1beta")
        );
    }

    #[test]
    fn env_bootstrap_applies_custom_video_understanding_base_url() {
        let _env = EnvGuard::new();
        std::env::set_var("CERUL_VIDEO_UNDERSTANDING_API_KEY", "test-key");
        std::env::set_var(
            "CERUL_VIDEO_UNDERSTANDING_BASE_URL",
            "https://gemini.example/v1beta/",
        );
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();

        bootstrap_env_providers(&paths).unwrap();

        let provider =
            cerul_storage::providers::get_provider(&paths, ENV_VIDEO_UNDERSTANDING_PROVIDER_ID)
                .unwrap()
                .unwrap();
        assert_eq!(provider.provider_type, "gemini");
        assert_eq!(
            provider.base_url.as_deref(),
            Some("https://gemini.example/v1beta")
        );
    }
}
