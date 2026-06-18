use std::{
    fs,
    io::{Read, Write},
    path::Path,
    process::Command,
    sync::Mutex,
    time::Duration,
};

use anyhow::Context;
use cerul_pipeline::mlx_sidecar::{
    external_runtime_dir, external_runtime_manifest_from_env, external_runtime_ready_marker,
    normalize_runtime_sha256, prepared_external_runtime_python_for_manifest, MlxSidecarConfig,
};
use cerul_storage::AppPaths;
use sha2::{Digest, Sha256};

static EXTERNAL_RUNTIME_LOCK: Mutex<()> = Mutex::new(());

pub fn ensure_external_mlx_runtime(
    paths: &AppPaths,
    config: &mut MlxSidecarConfig,
) -> anyhow::Result<()> {
    if config.python.is_file() {
        return Ok(());
    }

    let Some((manifest_path, manifest)) = external_runtime_manifest_from_env()? else {
        return Ok(());
    };

    let host_platform = host_runtime_platform();
    anyhow::ensure!(
        manifest.platform == host_platform,
        "external MLX runtime manifest platform {} does not match host {}",
        manifest.platform,
        host_platform
    );

    let _guard = EXTERNAL_RUNTIME_LOCK
        .lock()
        .map_err(|_| anyhow::anyhow!("external MLX runtime lock poisoned"))?;

    if let Some(python) = prepared_external_runtime_python_for_manifest(paths, &manifest) {
        config.python = python;
        return Ok(());
    }

    let digest = normalize_runtime_sha256(&manifest.sha256).ok_or_else(|| {
        anyhow::anyhow!("invalid MLX runtime sha256 in {}", manifest_path.display())
    })?;
    let archive_name = safe_archive_name(&manifest.archive)?;
    let downloads_dir = paths.data.join("runtimes").join("mlx").join("downloads");
    fs::create_dir_all(&downloads_dir)?;
    let archive_path = downloads_dir.join(archive_name);

    if !archive_matches_digest(&archive_path, &digest)? {
        download_runtime_archive(&manifest.url, manifest.size, &archive_path)?;
    }
    verify_archive_digest(&archive_path, &digest)?;

    let runtime_dir = external_runtime_dir(paths, &digest);
    let tmp_dir = runtime_dir.with_file_name(format!(
        "{}.tmp-{}",
        runtime_dir
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("runtime"),
        std::process::id()
    ));
    fs::remove_dir_all(&runtime_dir).ok();
    fs::remove_dir_all(&tmp_dir).ok();
    fs::create_dir_all(&tmp_dir)?;

    let extract_result = extract_runtime_archive(&archive_path, &tmp_dir)
        .and_then(|_| strip_quarantine_xattrs(&tmp_dir))
        .and_then(|_| {
            let python = tmp_dir.join("bin").join("python3");
            anyhow::ensure!(
                python.is_file(),
                "external MLX runtime archive did not contain bin/python3"
            );
            write_ready_marker(&tmp_dir, &digest)
        });

    if let Err(error) = extract_result {
        fs::remove_dir_all(&tmp_dir).ok();
        return Err(error);
    }

    fs::create_dir_all(
        runtime_dir
            .parent()
            .context("external runtime directory has no parent")?,
    )?;
    fs::rename(&tmp_dir, &runtime_dir)?;
    prune_old_external_runtimes(
        runtime_dir
            .parent()
            .context("external runtime directory has no parent")?,
        runtime_dir
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default(),
    );

    config.python = runtime_dir.join("bin").join("python3");
    Ok(())
}

fn host_runtime_platform() -> String {
    if std::env::consts::OS == "macos" && std::env::consts::ARCH == "aarch64" {
        "darwin-arm64".to_string()
    } else {
        format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH)
    }
}

fn safe_archive_name(name: &str) -> anyhow::Result<&str> {
    let path = Path::new(name);
    anyhow::ensure!(
        path.file_name().and_then(|file| file.to_str()) == Some(name),
        "external MLX runtime archive name must not contain path separators"
    );
    anyhow::ensure!(
        name.ends_with(".tar.gz"),
        "external MLX runtime archive must be a .tar.gz file"
    );
    Ok(name)
}

fn download_runtime_archive(
    url: &str,
    expected_size: u64,
    destination: &Path,
) -> anyhow::Result<()> {
    let tmp = destination.with_extension("download");
    fs::remove_file(&tmp).ok();
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(30 * 60))
        .build()?;
    let mut response = client
        .get(url)
        .send()
        .with_context(|| format!("failed to download external MLX runtime from {url}"))?
        .error_for_status()
        .with_context(|| format!("external MLX runtime download returned an error from {url}"))?;
    let mut file = fs::File::create(&tmp)?;
    let copied = response.copy_to(&mut file)?;
    file.flush()?;
    if expected_size > 0 {
        anyhow::ensure!(
            copied == expected_size,
            "external MLX runtime download size mismatch: expected {} bytes, got {} bytes",
            expected_size,
            copied
        );
    }
    fs::rename(tmp, destination)?;
    Ok(())
}

fn archive_matches_digest(path: &Path, digest: &str) -> anyhow::Result<bool> {
    if !path.is_file() {
        return Ok(false);
    }
    Ok(file_sha256(path)? == digest)
}

fn verify_archive_digest(path: &Path, digest: &str) -> anyhow::Result<()> {
    let actual = file_sha256(path)?;
    anyhow::ensure!(
        actual == digest,
        "external MLX runtime sha256 mismatch: expected {}, got {}",
        digest,
        actual
    );
    Ok(())
}

fn file_sha256(path: &Path) -> anyhow::Result<String> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 1024 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex_lower(&hasher.finalize()))
}

fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

fn extract_runtime_archive(archive: &Path, destination: &Path) -> anyhow::Result<()> {
    let output = Command::new("/usr/bin/tar")
        .arg("-xzf")
        .arg(archive)
        .arg("-C")
        .arg(destination)
        .output()
        .context("failed to launch tar for external MLX runtime extraction")?;
    anyhow::ensure!(
        output.status.success(),
        "failed to extract external MLX runtime archive: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}

fn strip_quarantine_xattrs(dir: &Path) -> anyhow::Result<()> {
    let output = Command::new("/usr/bin/xattr")
        .arg("-dr")
        .arg("com.apple.quarantine")
        .arg(dir)
        .output();
    match output {
        Ok(output) if output.status.success() => Ok(()),
        Ok(output) => {
            tracing::warn!(
                stderr = %String::from_utf8_lossy(&output.stderr),
                "failed to strip quarantine xattrs from external MLX runtime"
            );
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).context("failed to launch xattr for external MLX runtime"),
    }
}

fn write_ready_marker(runtime_dir: &Path, digest: &str) -> anyhow::Result<()> {
    let marker = runtime_dir.join(external_runtime_ready_marker());
    fs::write(
        marker,
        format!(
            "{}\n",
            serde_json::json!({
                "archive_sha256": digest,
                "created_at": chrono_like_timestamp(),
            })
        ),
    )?;
    Ok(())
}

fn chrono_like_timestamp() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs().to_string())
        .unwrap_or_else(|_| "0".to_string())
}

fn prune_old_external_runtimes(runtimes_root: &Path, keep_name: &str) {
    let Ok(entries) = fs::read_dir(runtimes_root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if name == keep_name || name == "downloads" || !path.is_dir() {
            continue;
        }
        fs::remove_dir_all(path).ok();
    }
}
