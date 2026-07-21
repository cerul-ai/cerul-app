use std::{
    env, fs,
    path::{Path, PathBuf},
    time::SystemTime,
};

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    inject_app_version();
    println!("cargo:rerun-if-env-changed=ZVEC_ROOT");
    println!("cargo:rerun-if-env-changed=ZVEC_LIB_DIR");
    println!("cargo:rerun-if-env-changed=ZVEC_BUNDLED_WHEEL_PATH");
    println!("cargo:rerun-if-env-changed=ZVEC_BUNDLED_WHEEL_URL");
    println!("cargo:rerun-if-env-changed=ZVEC_BUNDLED_WHEEL_SHA256");

    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let target_arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    reject_unsupported_zvec_runtime_target(&target_os, &target_arch);

    match target_os.as_str() {
        "macos" => println!("cargo:rustc-link-arg-bin=cerul-api=-Wl,-rpath,@loader_path"),
        "linux" => println!("cargo:rustc-link-arg-bin=cerul-api=-Wl,-rpath,$ORIGIN"),
        _ => {}
    }

    if let Err(error) = stage_zvec_runtime_library(&target_os) {
        println!("cargo:warning=failed to stage zvec runtime library next to cerul-api: {error}");
    }
}

fn inject_app_version() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let app_manifest = manifest_dir.join("../../package.json");
    println!("cargo:rerun-if-changed={}", app_manifest.display());

    let contents = fs::read_to_string(&app_manifest).expect("read root package.json");
    let package: serde_json::Value =
        serde_json::from_str(&contents).expect("parse root package.json");
    let version = package["version"]
        .as_str()
        .expect("root package.json must contain a string version");
    println!("cargo:rustc-env=CERUL_APP_VERSION={version}");
}

fn reject_unsupported_zvec_runtime_target(target_os: &str, target_arch: &str) {
    if target_os == "macos" && target_arch == "x86_64" && !has_zvec_runtime_override() {
        panic!(
            "x86_64-apple-darwin is not supported by zvec's bundled runtime wheels. \
             Provide a matching zvec runtime through ZVEC_ROOT/ZVEC_LIB_DIR, \
             ZVEC_BUNDLED_WHEEL_PATH, or ZVEC_BUNDLED_WHEEL_URL plus ZVEC_BUNDLED_WHEEL_SHA256."
        );
    }
    if target_os == "windows" && !has_zvec_runtime_override() {
        panic!(
            "Windows zvec builds require an explicit runtime override. \
             Provide a matching zvec runtime through ZVEC_ROOT/ZVEC_LIB_DIR, \
             ZVEC_BUNDLED_WHEEL_PATH, or ZVEC_BUNDLED_WHEEL_URL plus ZVEC_BUNDLED_WHEEL_SHA256."
        );
    }
}

fn has_zvec_runtime_override() -> bool {
    env::var_os("ZVEC_ROOT").is_some()
        || env::var_os("ZVEC_LIB_DIR").is_some()
        || env::var_os("ZVEC_BUNDLED_WHEEL_PATH").is_some()
        || (env::var_os("ZVEC_BUNDLED_WHEEL_URL").is_some()
            && env::var_os("ZVEC_BUNDLED_WHEEL_SHA256").is_some())
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
    if source != destination {
        fs::copy(&source, &destination).map_err(|err| {
            format!(
                "copy {} to {}: {err}",
                source.display(),
                destination.display()
            )
        })?;
    }
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
    for candidate in zvec_runtime_override_candidates(file_name) {
        if candidate.is_file() {
            return Ok(Some(candidate));
        }
    }

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
    if let Some(candidate) = newest_file(candidates)? {
        return Ok(Some(candidate));
    }

    let direct = target_dir.join(file_name);
    if direct.is_file() {
        return Ok(Some(direct));
    }
    Ok(None)
}

fn newest_file(candidates: Vec<PathBuf>) -> Result<Option<PathBuf>, String> {
    let mut newest = None::<(PathBuf, SystemTime)>;
    for candidate in candidates {
        let modified = fs::metadata(&candidate)
            .and_then(|metadata| metadata.modified())
            .map_err(|err| format!("read modified time for {}: {err}", candidate.display()))?;
        if newest
            .as_ref()
            .is_none_or(|(_, newest_modified)| modified > *newest_modified)
        {
            newest = Some((candidate, modified));
        }
    }
    Ok(newest.map(|(candidate, _)| candidate))
}

fn zvec_runtime_override_candidates(file_name: &str) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(dir) = env::var_os("ZVEC_LIB_DIR") {
        candidates.push(PathBuf::from(dir).join(file_name));
    }
    if let Some(root) = env::var_os("ZVEC_ROOT") {
        let root = PathBuf::from(root);
        candidates.push(root.join("lib").join(file_name));
        candidates.push(root.join("lib64").join(file_name));
    }
    candidates
}
