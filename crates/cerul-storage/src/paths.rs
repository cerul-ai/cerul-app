use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use directories::{BaseDirs, ProjectDirs};

const APP_DATA_DIR_NAME: &str = "Cerul";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppPaths {
    pub data: PathBuf,
    pub db: PathBuf,
    pub vector_index: PathBuf,
    pub models: PathBuf,
    pub cache: PathBuf,
}

impl AppPaths {
    pub fn resolve() -> anyhow::Result<Self> {
        let data = preferred_data_dir()?;
        migrate_legacy_data_dirs(&data)?;
        Self::from_data_dir(data)
    }

    pub fn from_data_dir(data: impl AsRef<Path>) -> anyhow::Result<Self> {
        let data = data.as_ref().to_path_buf();
        let paths = Self {
            db: data.join("cerul.db"),
            vector_index: data.join("indexes").join("zvec"),
            models: data.join("models"),
            cache: data.join("cache"),
            data,
        };

        remove_empty_legacy_lance_dir(&paths)?;
        for dir in [
            &paths.data,
            &paths.vector_index,
            &paths.models,
            &paths.cache,
        ] {
            fs::create_dir_all(dir)?;
        }

        Ok(paths)
    }

    pub fn source_cache_dir(&self, source_type: &str) -> PathBuf {
        self.cache.join("sources").join(source_type)
    }

    pub fn logs_dir(&self) -> PathBuf {
        self.data.join("logs")
    }
}

fn preferred_data_dir() -> anyhow::Result<PathBuf> {
    if let Some(path) = std::env::var_os("CERUL_DATA_DIR") {
        return Ok(PathBuf::from(path));
    }

    let base =
        BaseDirs::new().ok_or_else(|| anyhow::anyhow!("could not resolve Cerul data directory"))?;
    Ok(base.data_dir().join(APP_DATA_DIR_NAME))
}

fn migrate_legacy_data_dirs(preferred: &Path) -> anyhow::Result<()> {
    for legacy in legacy_data_dirs() {
        if legacy == preferred || !legacy.exists() {
            continue;
        }

        let legacy_score = directory_size(&legacy)?;
        if legacy_score == 0 {
            continue;
        }

        let preferred_score = directory_size(preferred).unwrap_or(0);
        if preferred.exists() && preferred_score >= legacy_score {
            continue;
        }

        if let Some(parent) = preferred.parent() {
            fs::create_dir_all(parent)?;
        }

        if preferred.exists() {
            let backup = backup_path(preferred);
            fs::rename(preferred, &backup)?;
        }

        fs::rename(&legacy, preferred)?;
    }

    Ok(())
}

fn legacy_data_dirs() -> Vec<PathBuf> {
    ProjectDirs::from("ai", "Cerul", "Cerul")
        .map(|proj| vec![proj.data_dir().to_path_buf()])
        .unwrap_or_default()
}

fn remove_empty_legacy_lance_dir(paths: &AppPaths) -> anyhow::Result<()> {
    let legacy = paths.data.join("lance");
    if !legacy.exists() {
        return Ok(());
    }

    let legacy_score = directory_size(&legacy)?;
    if legacy_score == 0 {
        fs::remove_dir_all(&legacy)?;
    }
    Ok(())
}

fn backup_path(path: &Path) -> PathBuf {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(APP_DATA_DIR_NAME);
    let mut candidate = path.with_file_name(format!("{file_name}.storage-backup-{timestamp}"));
    for index in 1.. {
        if !candidate.exists() {
            return candidate;
        }
        candidate = path.with_file_name(format!("{file_name}.storage-backup-{timestamp}-{index}"));
    }
    unreachable!("backup path loop always returns")
}

fn directory_size(path: &Path) -> std::io::Result<u64> {
    if !path.exists() {
        return Ok(0);
    }

    let mut total = 0_u64;
    let mut stack = vec![path.to_path_buf()];
    while let Some(current) = stack.pop() {
        for entry in fs::read_dir(current)? {
            let entry = entry?;
            let metadata = entry.metadata()?;
            if metadata.is_dir() {
                stack.push(entry.path());
            } else if metadata.is_file() {
                total = total.saturating_add(metadata.len());
            }
        }
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::AppPaths;

    #[test]
    fn from_data_dir_uses_indexes_zvec_layout() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_data_dir(temp.path()).unwrap();

        assert_eq!(paths.db, temp.path().join("cerul.db"));
        assert_eq!(paths.models, temp.path().join("models"));
        assert_eq!(paths.cache, temp.path().join("cache"));
        assert_eq!(paths.vector_index, temp.path().join("indexes").join("zvec"));
        assert_eq!(
            paths.source_cache_dir("youtube"),
            temp.path().join("cache").join("sources").join("youtube")
        );
        assert_eq!(paths.logs_dir(), temp.path().join("logs"));
    }

    #[test]
    fn from_data_dir_leaves_non_empty_legacy_lance_directory_in_place() {
        let temp = tempfile::tempdir().unwrap();
        let legacy_lance = temp.path().join("lance");
        std::fs::create_dir_all(&legacy_lance).unwrap();
        std::fs::write(legacy_lance.join("table.lance"), b"vectors").unwrap();

        let paths = AppPaths::from_data_dir(temp.path()).unwrap();

        assert!(temp.path().join("lance").join("table.lance").is_file());
        assert!(paths.vector_index.is_dir());
    }
}
