use std::{
    env, fs,
    path::{Path, PathBuf},
};

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    match target_os.as_str() {
        "macos" => println!("cargo:rustc-link-arg-bin=cerul-api=-Wl,-rpath,@loader_path"),
        "linux" => println!("cargo:rustc-link-arg-bin=cerul-api=-Wl,-rpath,$ORIGIN"),
        _ => {}
    }

    if let Err(error) = stage_zvec_runtime_library(&target_os) {
        println!("cargo:warning=failed to stage zvec runtime library next to cerul-api: {error}");
    }
}

fn stage_zvec_runtime_library(target_os: &str) -> Result<(), String> {
    let Some(file_name) = zvec_runtime_library_name(target_os) else {
        return Ok(());
    };
    let target_dir = target_dir()?;
    let Some(source) = find_zvec_runtime_library(&target_dir, file_name)? else {
        println!("cargo:warning=zvec runtime library {file_name} was not found under target build outputs");
        return Ok(());
    };
    let destination = target_dir.join(file_name);
    fs::copy(&source, &destination).map_err(|err| {
        format!(
            "copy {} to {}: {err}",
            source.display(),
            destination.display()
        )
    })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&destination, fs::Permissions::from_mode(0o755)).map_err(|err| {
            format!(
                "set executable permissions on {}: {err}",
                destination.display()
            )
        })?;
    }
    Ok(())
}

fn zvec_runtime_library_name(target_os: &str) -> Option<&'static str> {
    match target_os {
        "macos" => Some("libzvec_c_api.dylib"),
        "linux" => Some("libzvec_c_api.so"),
        "windows" => Some("zvec_c_api.dll"),
        _ => None,
    }
}

fn target_dir() -> Result<PathBuf, String> {
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").ok_or("OUT_DIR is not set")?);
    out_dir
        .ancestors()
        .nth(3)
        .map(Path::to_path_buf)
        .ok_or_else(|| format!("could not resolve target dir from {}", out_dir.display()))
}

fn find_zvec_runtime_library(
    target_dir: &Path,
    file_name: &str,
) -> Result<Option<PathBuf>, String> {
    let build_dir = target_dir.join("build");
    let entries = match fs::read_dir(&build_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("read {}: {error}", build_dir.display())),
    };

    let mut candidates = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|err| format!("read entry in {}: {err}", build_dir.display()))?;
        let path = entry.path();
        if !path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("zvec-"))
        {
            continue;
        }
        let candidate = path
            .join("out")
            .join("zvec-bundled")
            .join("lib")
            .join(file_name);
        if candidate.is_file() {
            candidates.push(candidate);
        }
    }
    candidates.sort();
    Ok(candidates.pop())
}
