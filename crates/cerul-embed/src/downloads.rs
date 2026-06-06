use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use serde::Deserialize;

use crate::QWEN3_VL_REPO_ID;

const HF_DIRECT_ENDPOINT: &str = "https://huggingface.co";
const HF_MIRROR_ENDPOINT: &str = "https://hf-mirror.com";
const MODELSCOPE_ENDPOINT: &str = "https://modelscope.cn";
const CERUL_PREFETCH_REF: &str = "cerul-prefetch";
// Files every complete Qwen3-VL-Embedding snapshot must contain. `tokenizer_config.json`
// and `special_tokens_map.json` are included deliberately: an older Cerul prefetch only
// grabbed config/preprocessor/tokenizer + weights, so requiring these forces a re-fetch
// of those stale, incomplete caches.
const QWEN3_REQUIRED_FILES: &[&str] = &[
    "config.json",
    "preprocessor_config.json",
    "tokenizer.json",
    "tokenizer_config.json",
    "special_tokens_map.json",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelDownloadSource {
    Auto,
    HuggingFace,
    HuggingFaceMirror,
    ModelScope,
}

impl ModelDownloadSource {
    pub fn parse(value: &str) -> Self {
        match value.trim() {
            "huggingface" => Self::HuggingFace,
            "hf-mirror" => Self::HuggingFaceMirror,
            "modelscope" => Self::ModelScope,
            _ => Self::Auto,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::HuggingFace => "huggingface",
            Self::HuggingFaceMirror => "hf-mirror",
            Self::ModelScope => "modelscope",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelDownloadConfig {
    pub source: ModelDownloadSource,
    pub proxy_url: Option<String>,
    pub hf_endpoint: String,
    pub hf_mirror_endpoint: String,
    pub modelscope_endpoint: String,
}

impl Default for ModelDownloadConfig {
    fn default() -> Self {
        Self {
            source: ModelDownloadSource::Auto,
            proxy_url: None,
            hf_endpoint: HF_DIRECT_ENDPOINT.to_string(),
            hf_mirror_endpoint: HF_MIRROR_ENDPOINT.to_string(),
            modelscope_endpoint: MODELSCOPE_ENDPOINT.to_string(),
        }
    }
}

impl ModelDownloadConfig {
    pub fn with_source(mut self, source: ModelDownloadSource) -> Self {
        self.source = source;
        self
    }

    pub fn with_proxy_url(mut self, proxy_url: Option<String>) -> Self {
        self.proxy_url = clean_optional(proxy_url);
        self
    }

    pub fn effective_hf_endpoint(&self) -> &str {
        match self.source {
            ModelDownloadSource::HuggingFace | ModelDownloadSource::Auto => &self.hf_endpoint,
            ModelDownloadSource::HuggingFaceMirror | ModelDownloadSource::ModelScope => {
                &self.hf_mirror_endpoint
            }
        }
    }
}

#[derive(Debug, Clone)]
struct RemoteFile {
    path: String,
    url: String,
}

#[derive(Debug, Deserialize)]
struct HfModelInfo {
    siblings: Vec<HfSibling>,
}

#[derive(Debug, Deserialize)]
struct HfSibling {
    rfilename: String,
}

#[derive(Debug, Deserialize)]
struct ModelScopeFilesResponse {
    #[serde(rename = "Data", alias = "data")]
    data: Option<ModelScopeData>,
    #[serde(rename = "Message", alias = "message")]
    message: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum ModelScopeData {
    Nested {
        #[serde(rename = "Files", alias = "files")]
        files: Vec<ModelScopeFile>,
    },
    Direct(Vec<ModelScopeFile>),
}

impl ModelScopeData {
    fn into_files(self) -> Vec<ModelScopeFile> {
        match self {
            Self::Nested { files } | Self::Direct(files) => files,
        }
    }
}

#[derive(Debug, Deserialize)]
struct ModelScopeFile {
    #[serde(rename = "Path", alias = "path")]
    path: Option<String>,
    #[serde(rename = "Name", alias = "name")]
    name: Option<String>,
    #[serde(rename = "Type", alias = "type")]
    file_type: Option<String>,
}

pub fn apply_download_env(models_dir: &Path, config: &ModelDownloadConfig) -> anyhow::Result<()> {
    let hf_home = models_dir.join("huggingface");
    let fastembed_cache = models_dir.join("fastembed");
    fs::create_dir_all(&hf_home)?;
    fs::create_dir_all(&fastembed_cache)?;

    if std::env::var_os("HF_HOME").is_none() {
        std::env::set_var("HF_HOME", &hf_home);
    }
    if std::env::var_os("FASTEMBED_CACHE_DIR").is_none() {
        std::env::set_var("FASTEMBED_CACHE_DIR", &fastembed_cache);
    }
    if config.source == ModelDownloadSource::Auto && std::env::var_os("HF_ENDPOINT").is_none() {
        std::env::set_var("HF_ENDPOINT", config.effective_hf_endpoint());
    } else if config.source != ModelDownloadSource::Auto {
        std::env::set_var("HF_ENDPOINT", config.effective_hf_endpoint());
    }
    Ok(())
}

pub fn ensure_qwen3_vl_prefetched(
    models_dir: &Path,
    config: &ModelDownloadConfig,
) -> anyhow::Result<()> {
    let app_hub = models_dir.join("huggingface").join("hub");
    let default_hub = default_hf_hub_cache();
    if default_hub
        .as_ref()
        .is_some_and(|hub| qwen_cache_has_required_files(hub))
    {
        return Ok(());
    }
    if qwen_cache_has_required_files(&app_hub) {
        link_qwen_cache_into_default(&app_hub, default_hub.as_deref())?;
        return Ok(());
    }

    let client = download_client(config)?;
    let remotes = resolve_qwen3_files(&client, config)?;
    prefetch_into_hf_cache(&client, &app_hub, &remotes)?;
    link_qwen_cache_into_default(&app_hub, default_hub.as_deref())?;
    Ok(())
}

pub fn qwen_cache_size_bytes(models_dir: &Path) -> u64 {
    let app_hub = models_dir.join("huggingface").join("hub");
    if let Some(default_hub) = default_hf_hub_cache() {
        if default_hub != app_hub {
            return dir_size_bytes(&qwen_repo_root(&default_hub)).unwrap_or(0);
        }
    }
    0
}

pub fn download_client(config: &ModelDownloadConfig) -> anyhow::Result<reqwest::blocking::Client> {
    let mut builder = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(600))
        .connect_timeout(std::time::Duration::from_secs(30))
        .user_agent(format!(
            "Cerul/{} model-downloader",
            env!("CARGO_PKG_VERSION")
        ));
    if let Some(proxy_url) = &config.proxy_url {
        builder = builder.proxy(reqwest::Proxy::all(proxy_url)?);
    }
    Ok(builder.build()?)
}

fn resolve_qwen3_files(
    client: &reqwest::blocking::Client,
    config: &ModelDownloadConfig,
) -> anyhow::Result<Vec<RemoteFile>> {
    let mut errors = Vec::new();
    for source in source_order(config.source) {
        let result = match source {
            ModelDownloadSource::ModelScope => list_modelscope_qwen_files(client, config),
            ModelDownloadSource::HuggingFaceMirror => {
                list_hf_qwen_files(client, &config.hf_mirror_endpoint)
            }
            ModelDownloadSource::HuggingFace => list_hf_qwen_files(client, &config.hf_endpoint),
            ModelDownloadSource::Auto => unreachable!("source_order expands auto"),
        };
        match result {
            Ok(files) => return Ok(files),
            Err(error) => errors.push(format!("{}: {error}", source.as_str())),
        }
    }
    anyhow::bail!(
        "could not resolve Qwen3-VL model files ({})",
        errors.join("; ")
    )
}

fn source_order(source: ModelDownloadSource) -> Vec<ModelDownloadSource> {
    match source {
        ModelDownloadSource::Auto => vec![
            ModelDownloadSource::HuggingFace,
            ModelDownloadSource::ModelScope,
            ModelDownloadSource::HuggingFaceMirror,
        ],
        other => vec![other],
    }
}

fn list_hf_qwen_files(
    client: &reqwest::blocking::Client,
    endpoint: &str,
) -> anyhow::Result<Vec<RemoteFile>> {
    let endpoint = endpoint.trim_end_matches('/');
    let info_url = format!("{endpoint}/api/models/{QWEN3_VL_REPO_ID}/revision/main");
    let info: HfModelInfo = client.get(info_url).send()?.error_for_status()?.json()?;
    let filenames = info
        .siblings
        .into_iter()
        .map(|sibling| sibling.rfilename)
        .collect::<Vec<_>>();
    let selected = select_qwen_files(&filenames)?;
    Ok(selected
        .into_iter()
        .map(|path| RemoteFile {
            url: format!("{endpoint}/{QWEN3_VL_REPO_ID}/resolve/main/{path}"),
            path,
        })
        .collect())
}

fn list_modelscope_qwen_files(
    client: &reqwest::blocking::Client,
    config: &ModelDownloadConfig,
) -> anyhow::Result<Vec<RemoteFile>> {
    let endpoint = config.modelscope_endpoint.trim_end_matches('/');
    let mut last_error = None;
    let response = [
        format!(
            "{endpoint}/api/v1/models/{QWEN3_VL_REPO_ID}/repo/files?Revision=master&Recursive=true"
        ),
        format!(
            "{endpoint}/api/v1/models/{QWEN3_VL_REPO_ID}/repo/files?revision=master&recursive=true"
        ),
    ]
    .into_iter()
    .find_map(|files_url| {
        match client
            .get(&files_url)
            .send()
            .and_then(|res| res.error_for_status())
        {
            Ok(response) => match response.json::<ModelScopeFilesResponse>() {
                Ok(parsed) => Some(parsed),
                Err(error) => {
                    last_error = Some(format!("{files_url}: {error}"));
                    None
                }
            },
            Err(error) => {
                last_error = Some(format!("{files_url}: {error}"));
                None
            }
        }
    })
    .ok_or_else(|| {
        anyhow::anyhow!(
            "ModelScope file list request failed{}",
            last_error
                .as_deref()
                .map(|error| format!(": {error}"))
                .unwrap_or_default()
        )
    })?;
    let filenames = response
        .data
        .ok_or_else(|| {
            anyhow::anyhow!(
                "ModelScope did not return file data{}",
                response
                    .message
                    .as_deref()
                    .map(|message| format!(": {message}"))
                    .unwrap_or_default()
            )
        })?
        .into_files()
        .into_iter()
        .filter(|file| !matches!(file.file_type.as_deref(), Some("tree" | "dir" | "folder")))
        .filter_map(|file| file.path.or(file.name))
        .collect::<Vec<_>>();
    let selected = select_qwen_files(&filenames)?;
    Ok(selected
        .into_iter()
        .map(|path| RemoteFile {
            url: format!("{endpoint}/models/{QWEN3_VL_REPO_ID}/resolve/master/{path}"),
            path,
        })
        .collect())
}

fn select_qwen_files(filenames: &[String]) -> anyhow::Result<Vec<String>> {
    for required in QWEN3_REQUIRED_FILES {
        anyhow::ensure!(
            filenames.iter().any(|name| name == required),
            "Qwen3-VL repository is missing {required}"
        );
    }
    anyhow::ensure!(
        filenames.iter().any(|name| is_qwen_weight_file(name)),
        "Qwen3-VL repository does not expose model.safetensors or sharded safetensors"
    );

    // Fetch the whole repo (config, tokenizer, pooling, and weight files) so the
    // loader never has to fall back to a network download for a missing file.
    let mut selected = filenames
        .iter()
        .filter(|name| !is_excluded_qwen_file(name))
        .cloned()
        .collect::<Vec<_>>();
    selected.sort();
    Ok(selected)
}

fn is_excluded_qwen_file(name: &str) -> bool {
    name == ".gitattributes" || name.to_ascii_lowercase().ends_with(".md")
}

fn is_qwen_weight_file(name: &str) -> bool {
    name == "model.safetensors"
        || (name.starts_with("model-") && name.ends_with(".safetensors") && name.contains("-of-"))
}

fn prefetch_into_hf_cache(
    client: &reqwest::blocking::Client,
    hub_root: &Path,
    files: &[RemoteFile],
) -> anyhow::Result<()> {
    let snapshot_dir = qwen_snapshot_dir(hub_root);
    fs::create_dir_all(&snapshot_dir)?;

    for remote in files {
        let destination = snapshot_dir.join(&remote.path);
        if destination.is_file() {
            continue;
        }
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        let temp = destination.with_extension("download");
        let mut response = client.get(&remote.url).send()?.error_for_status()?;
        let mut file = fs::File::create(&temp)?;
        std::io::copy(&mut response, &mut file)?;
        file.flush()?;
        drop(file);
        fs::rename(temp, destination)?;
    }

    create_qwen_ref(hub_root)
}

fn link_qwen_cache_into_default(app_hub: &Path, default_hub: Option<&Path>) -> anyhow::Result<()> {
    let Some(default_hub) = default_hub else {
        return Ok(());
    };
    if default_hub == app_hub {
        return Ok(());
    }

    let app_snapshot = qwen_snapshot_dir_for_main_ref(app_hub)
        .unwrap_or_else(|| qwen_prefetch_snapshot_dir(app_hub));
    let default_snapshot = qwen_snapshot_dir(default_hub);
    fs::create_dir_all(&default_snapshot)?;

    for path in collect_files(&app_snapshot)? {
        let relative = path.strip_prefix(&app_snapshot)?;
        let destination = default_snapshot.join(relative);
        if destination.exists() {
            continue;
        }
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        link_or_copy_file(&path, &destination)?;
    }
    create_qwen_ref(default_hub)?;
    Ok(())
}

fn create_qwen_ref(hub_root: &Path) -> anyhow::Result<()> {
    let ref_path = qwen_repo_root(hub_root).join("refs").join("main");
    if let Some(parent) = ref_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(ref_path, CERUL_PREFETCH_REF)?;
    Ok(())
}

fn qwen_cache_has_required_files(hub_root: &Path) -> bool {
    let Some(snapshot_dir) = qwen_snapshot_dir_for_main_ref(hub_root) else {
        return false;
    };
    QWEN3_REQUIRED_FILES
        .iter()
        .all(|file| snapshot_dir.join(file).is_file())
        && (snapshot_dir.join("model.safetensors").is_file()
            || collect_files(&snapshot_dir)
                .map(|files| {
                    files.iter().any(|path| {
                        path.file_name()
                            .and_then(|name| name.to_str())
                            .is_some_and(|name| {
                                name.starts_with("model-")
                                    && name.contains("-of-")
                                    && name.ends_with(".safetensors")
                            })
                    })
                })
                .unwrap_or(false))
}

fn qwen_repo_root(hub_root: &Path) -> PathBuf {
    hub_root.join("models--Qwen--Qwen3-VL-Embedding-2B")
}

fn qwen_snapshot_dir(hub_root: &Path) -> PathBuf {
    qwen_prefetch_snapshot_dir(hub_root)
}

fn qwen_prefetch_snapshot_dir(hub_root: &Path) -> PathBuf {
    qwen_repo_root(hub_root)
        .join("snapshots")
        .join(CERUL_PREFETCH_REF)
}

fn qwen_snapshot_dir_for_main_ref(hub_root: &Path) -> Option<PathBuf> {
    let repo_root = qwen_repo_root(hub_root);
    let ref_name = fs::read_to_string(repo_root.join("refs").join("main")).ok()?;
    let ref_name = ref_name.trim();
    if ref_name.is_empty() {
        return None;
    }
    Some(repo_root.join("snapshots").join(ref_name))
}

fn default_hf_hub_cache() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|home| {
        PathBuf::from(home)
            .join(".cache")
            .join("huggingface")
            .join("hub")
    })
}

fn collect_files(root: &Path) -> anyhow::Result<Vec<PathBuf>> {
    if !root.exists() {
        return Ok(Vec::new());
    }
    let mut files = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(current) = stack.pop() {
        for entry in fs::read_dir(current)? {
            let entry = entry?;
            let file_type = entry.file_type()?;
            if file_type.is_dir() {
                stack.push(entry.path());
            } else if file_type.is_file() {
                files.push(entry.path());
            }
        }
    }
    Ok(files)
}

fn dir_size_bytes(path: &Path) -> std::io::Result<u64> {
    let mut total = 0u64;
    for file in collect_files(path).unwrap_or_default() {
        total = total.saturating_add(file.metadata().map(|meta| meta.len()).unwrap_or(0));
    }
    Ok(total)
}

#[cfg(unix)]
fn link_or_copy_file(source: &Path, destination: &Path) -> anyhow::Result<()> {
    // Drop any existing entry first. A stale/dangling symlink — e.g. left by an
    // older app-data directory path — otherwise makes symlink/hard_link fail with
    // EEXIST and copy fail trying to write through the broken link.
    let _ = fs::remove_file(destination);
    std::os::unix::fs::symlink(source, destination)
        .or_else(|_| fs::hard_link(source, destination))
        .or_else(|_| fs::copy(source, destination).map(|_| ()))?;
    Ok(())
}

#[cfg(not(unix))]
fn link_or_copy_file(source: &Path, destination: &Path) -> anyhow::Result<()> {
    let _ = fs::remove_file(destination);
    fs::hard_link(source, destination).or_else(|_| fs::copy(source, destination).map(|_| ()))?;
    Ok(())
}

fn clean_optional(value: Option<String>) -> Option<String> {
    value
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_parser_defaults_to_auto() {
        assert_eq!(
            ModelDownloadSource::parse("modelscope"),
            ModelDownloadSource::ModelScope
        );
        assert_eq!(
            ModelDownloadSource::parse("hf-mirror"),
            ModelDownloadSource::HuggingFaceMirror
        );
        assert_eq!(ModelDownloadSource::parse(""), ModelDownloadSource::Auto);
    }

    #[test]
    fn auto_download_order_prefers_hugging_face_first() {
        assert_eq!(
            source_order(ModelDownloadSource::Auto),
            vec![
                ModelDownloadSource::HuggingFace,
                ModelDownloadSource::ModelScope,
                ModelDownloadSource::HuggingFaceMirror
            ]
        );
        assert_eq!(
            ModelDownloadConfig::default().effective_hf_endpoint(),
            HF_DIRECT_ENDPOINT
        );
    }

    fn full_qwen_repo_files() -> Vec<String> {
        [
            ".gitattributes",
            "1_Pooling/config.json",
            "README.md",
            "added_tokens.json",
            "chat_template.jinja",
            "config.json",
            "merges.txt",
            "model.safetensors",
            "preprocessor_config.json",
            "special_tokens_map.json",
            "tokenizer.json",
            "tokenizer_config.json",
            "vocab.json",
        ]
        .into_iter()
        .map(String::from)
        .collect()
    }

    #[test]
    fn select_qwen_files_keeps_model_files_and_drops_docs() {
        let selected = select_qwen_files(&full_qwen_repo_files()).unwrap();

        assert!(selected.contains(&"model.safetensors".to_string()));
        assert!(selected.contains(&"tokenizer_config.json".to_string()));
        assert!(selected.contains(&"1_Pooling/config.json".to_string()));
        assert!(!selected.contains(&"README.md".to_string()));
        assert!(!selected.contains(&".gitattributes".to_string()));
    }

    #[test]
    fn select_qwen_files_accepts_sharded_weights() {
        let mut files = full_qwen_repo_files();
        files.retain(|name| name != "model.safetensors");
        files.push("model-00001-of-00002.safetensors".to_string());
        files.push("model-00002-of-00002.safetensors".to_string());

        let selected = select_qwen_files(&files).unwrap();
        assert!(selected.contains(&"model-00001-of-00002.safetensors".to_string()));
        assert!(selected.contains(&"model-00002-of-00002.safetensors".to_string()));
    }

    #[test]
    fn select_qwen_files_rejects_repo_without_weights() {
        let mut files = full_qwen_repo_files();
        files.retain(|name| name != "model.safetensors");

        assert!(select_qwen_files(&files).is_err());
    }

    #[test]
    fn qwen_cache_accepts_main_ref_snapshot() {
        let temp = tempfile::tempdir().unwrap();
        let hub = temp.path();
        let snapshot = qwen_repo_root(hub).join("snapshots").join("abc123");
        write_required_qwen_files(&snapshot, "model-00001-of-00001.safetensors");
        let refs = qwen_repo_root(hub).join("refs");
        fs::create_dir_all(&refs).unwrap();
        fs::write(refs.join("main"), "abc123\n").unwrap();

        assert!(qwen_cache_has_required_files(hub));
    }

    #[test]
    fn link_qwen_cache_uses_existing_main_ref_snapshot() {
        let app_temp = tempfile::tempdir().unwrap();
        let default_temp = tempfile::tempdir().unwrap();
        let app_hub = app_temp.path();
        let default_hub = default_temp.path();
        let snapshot = qwen_repo_root(app_hub).join("snapshots").join("abc123");
        write_required_qwen_files(&snapshot, "model.safetensors");
        let app_refs = qwen_repo_root(app_hub).join("refs");
        fs::create_dir_all(&app_refs).unwrap();
        fs::write(app_refs.join("main"), "abc123\n").unwrap();

        link_qwen_cache_into_default(app_hub, Some(default_hub)).unwrap();

        assert_eq!(
            fs::read_to_string(app_refs.join("main")).unwrap(),
            "abc123\n"
        );
        assert_eq!(
            fs::read_to_string(qwen_repo_root(default_hub).join("refs").join("main")).unwrap(),
            CERUL_PREFETCH_REF
        );
        assert!(qwen_cache_has_required_files(default_hub));
    }

    fn write_required_qwen_files(snapshot: &Path, model_file: &str) {
        fs::create_dir_all(snapshot).unwrap();
        for required in QWEN3_REQUIRED_FILES {
            fs::write(snapshot.join(required), "test").unwrap();
        }
        fs::write(snapshot.join(model_file), "test").unwrap();
    }
}
