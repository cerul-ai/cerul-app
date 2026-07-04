use std::env;
use std::path::{Path, PathBuf};

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let vendor_header = manifest_dir.join("vendor").join("c_api.h");

    println!("cargo:rerun-if-changed=vendor/c_api.h");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=ZVEC_INCLUDE_DIR");
    println!("cargo:rerun-if-env-changed=ZVEC_LIB_DIR");
    println!("cargo:rerun-if-env-changed=ZVEC_ROOT");
    println!("cargo:rerun-if-env-changed=ZVEC_STATIC");
    println!("cargo:rerun-if-env-changed=ZVEC_BUNDLED_WHEEL_URL");
    println!("cargo:rerun-if-env-changed=ZVEC_BUNDLED_WHEEL_SHA256");
    println!("cargo:rerun-if-env-changed=ZVEC_BUNDLED_WHEEL_PATH");
    println!("cargo:rerun-if-env-changed=DOCS_RS");

    // Let downstream crates discover the vendored headers.
    println!("cargo:include={}", manifest_dir.join("vendor").display());

    // With the `bundled` feature, try to materialize a libzvec_c_api under
    // OUT_DIR before anything else. The resolved install prefix is also used
    // as the header source below.
    #[cfg(feature = "bundled")]
    let bundled_root = bundled::materialize();
    #[cfg(not(feature = "bundled"))]
    let bundled_root: Option<PathBuf> = None;

    let header_path = resolve_header(&manifest_dir, &vendor_header, bundled_root.as_deref());

    generate_bindings(&header_path);

    if env::var_os("DOCS_RS").is_some() {
        // docs.rs sandbox: skip linking, we only build for rustdoc.
        return;
    }

    configure_link(bundled_root.as_deref());
}

fn resolve_header(
    manifest_dir: &Path,
    vendor_header: &Path,
    bundled_root: Option<&Path>,
) -> PathBuf {
    // Prefer a user-specified include dir so bindings match their installed lib.
    if let Some(dir) = env::var_os("ZVEC_INCLUDE_DIR") {
        let candidate = PathBuf::from(dir).join("zvec").join("c_api.h");
        if candidate.exists() {
            return candidate;
        }
    }
    if let Some(root) = env::var_os("ZVEC_ROOT") {
        let candidate = PathBuf::from(root)
            .join("include")
            .join("zvec")
            .join("c_api.h");
        if candidate.exists() {
            return candidate;
        }
    }
    if let Some(root) = bundled_root {
        let candidate = root.join("include").join("zvec").join("c_api.h");
        if candidate.exists() {
            return candidate;
        }
    }

    assert!(
        vendor_header.exists(),
        "vendored header {} not found and no ZVEC_INCLUDE_DIR/ZVEC_ROOT override",
        vendor_header.display()
    );
    let _ = manifest_dir;
    vendor_header.to_path_buf()
}

fn generate_bindings(header: &Path) {
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());

    let bindings = bindgen::Builder::default()
        .header(header.to_string_lossy())
        .allowlist_function("zvec_.*")
        .allowlist_type("zvec_.*")
        .allowlist_var("ZVEC_.*")
        .default_enum_style(bindgen::EnumVariation::NewType {
            is_bitfield: false,
            is_global: false,
        })
        .constified_enum_module("zvec_error_code_t")
        .prepend_enum_name(false)
        .derive_debug(true)
        .derive_default(true)
        .generate_comments(true)
        .size_t_is_usize(true)
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        .generate()
        .expect("failed to generate zvec C API bindings");

    bindings
        .write_to_file(out_dir.join("bindings.rs"))
        .expect("failed to write bindings.rs");
}

fn configure_link(bundled_root: Option<&Path>) {
    // 1. Explicit ZVEC_LIB_DIR / ZVEC_ROOT takes precedence over everything.
    if let Some(dir) = env::var_os("ZVEC_LIB_DIR") {
        println!(
            "cargo:rustc-link-search=native={}",
            PathBuf::from(dir).display()
        );
        emit_link();
        return;
    }
    if let Some(root) = env::var_os("ZVEC_ROOT") {
        let lib = PathBuf::from(&root).join("lib");
        println!("cargo:rustc-link-search=native={}", lib.display());
        let lib64 = PathBuf::from(&root).join("lib64");
        if lib64.exists() {
            println!("cargo:rustc-link-search=native={}", lib64.display());
        }
        emit_link();
        return;
    }

    // 2. Bundled wheel extraction (if feature `bundled` is on and we were
    //    able to fetch + unpack one).
    if let Some(root) = bundled_root {
        let lib = root.join("lib");
        println!("cargo:rustc-link-search=native={}", lib.display());
        // Export the link search path to downstream -sys consumers and, on
        // Unix, set rpath so the resulting binaries find the .so without
        // LD_LIBRARY_PATH.
        println!("cargo:lib={}", lib.display());
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            let rpath = if cfg!(target_os = "macos") {
                "@loader_path"
            } else {
                "$ORIGIN"
            };
            println!("cargo:rustc-link-arg=-Wl,-rpath,{}", lib.display());
            let _ = rpath; // reserved for a future packaging mode
        }
        emit_link();
        return;
    }

    // 3. Optional pkg-config probe.
    #[cfg(feature = "pkg-config")]
    {
        if pkg_config::Config::new()
            .atleast_version("0.3.0")
            .probe("zvec_c_api")
            .is_ok()
        {
            return;
        }
    }

    // 4. Fall back to the system linker's default search paths.
    emit_link();
}

fn emit_link() {
    let kind = if env::var_os("ZVEC_STATIC").is_some() {
        "static"
    } else {
        "dylib"
    };
    println!("cargo:rustc-link-lib={}=zvec_c_api", kind);
}

#[cfg(feature = "bundled")]
mod bundled {
    //! Download + extract the upstream zvec PyPI wheel for the current
    //! target. Supported target triples:
    //!
    //! - `x86_64-unknown-linux-gnu`   → `manylinux_2_28_x86_64`
    //! - `aarch64-unknown-linux-gnu`  → `manylinux_2_28_aarch64`
    //! - `aarch64-apple-darwin`       → `macosx_11_0_arm64`
    //! - `x86_64-pc-windows-msvc`     → `win_amd64`
    //!
    //! All other targets return `None` (and the caller falls back to the
    //! regular linker discovery path).

    use std::env;
    use std::fs::{self, File};
    use std::io::{self, Read, Write};
    use std::path::{Path, PathBuf};

    use sha2::{Digest, Sha256};

    /// Pinned wheels for zvec 0.3.1, matching the header vendored at
    /// `vendor/c_api.h`. The cp311 ABI is an arbitrary choice — the
    /// extension module is Python-specific but the bundled
    /// `libzvec_c_api` / `c_api.h` are identical across cp310–cp314 for
    /// the same platform.
    struct Wheel {
        url: &'static str,
        sha256: &'static str,
    }

    fn select_wheel() -> Option<Wheel> {
        let target = env::var("TARGET").ok()?;
        Some(match target.as_str() {
            "x86_64-unknown-linux-gnu" => Wheel {
                url: "https://files.pythonhosted.org/packages/77/6f/5a7463fedb8fbb543c40297118627feeb1b5dc080b154cd779cdf3879827/zvec-0.3.1-cp311-cp311-manylinux_2_28_x86_64.whl",
                sha256: "ee2febd1cb872c023eeeeba17bac3fa7471a93b486ac14fb4afca62161a502b7",
            },
            "aarch64-unknown-linux-gnu" => Wheel {
                url: "https://files.pythonhosted.org/packages/17/4f/e23e2813b7515c75c28261315e24b34b1726163508dd9ee4629d8504233c/zvec-0.3.1-cp311-cp311-manylinux_2_28_aarch64.whl",
                sha256: "58f3411dc7f0a58a3c1c8d7c20689f7e41d4ad301933d6633fdcc971304e9cea",
            },
            "aarch64-apple-darwin" => Wheel {
                url: "https://files.pythonhosted.org/packages/15/0c/8a375b3f503d984fadc4777978f62e8f171cef94ee8e8e1816e8c8728326/zvec-0.3.1-cp311-cp311-macosx_11_0_arm64.whl",
                sha256: "fdc48fa0f58248596be194bd5fed5eb1dbbdf7daa8346ecd4cb88b0e6f7ff022",
            },
            "x86_64-pc-windows-msvc" => Wheel {
                url: "https://files.pythonhosted.org/packages/54/44/45a8b58ecbf0904183c8896cf774d11074768dc2e9b5c1e277f36328d976/zvec-0.3.1-cp311-cp311-win_amd64.whl",
                sha256: "aefc5f40e93474348c86f73f430709cb926f5234c1ca696e3f77cb73a5f041a1",
            },
            _ => return None,
        })
    }

    /// Fetch + extract the wheel into `$OUT_DIR/zvec-bundled/{lib,include}`
    /// and return that prefix. Returns `None` if the target is unsupported,
    /// in which case the caller continues with the regular linker
    /// discovery flow.
    pub(crate) fn materialize() -> Option<PathBuf> {
        // docs.rs sandbox: no network, no linking. Skip the wheel fetch so
        // the crate documents cleanly with `--all-features`.
        if env::var_os("DOCS_RS").is_some() {
            return None;
        }

        // Local override — useful for air-gapped builds or TLS-restricted
        // networks. If set, we trust the file is a matching zvec wheel.
        let local_wheel = env::var_os("ZVEC_BUNDLED_WHEEL_PATH").map(PathBuf::from);

        let wheel = match (
            env::var_os("ZVEC_BUNDLED_WHEEL_URL"),
            env::var_os("ZVEC_BUNDLED_WHEEL_SHA256"),
        ) {
            (Some(url), Some(sha)) => Wheel {
                url: Box::leak(url.into_string().ok()?.into_boxed_str()),
                sha256: Box::leak(sha.into_string().ok()?.into_boxed_str()),
            },
            _ => match select_wheel() {
                Some(w) => w,
                None => {
                    println!(
                        "cargo:warning=bundled feature: no prebuilt wheel for target {}; falling back to linker discovery",
                        env::var("TARGET").unwrap_or_default()
                    );
                    return None;
                }
            },
        };

        let out_dir = PathBuf::from(env::var_os("OUT_DIR")?);
        let prefix = out_dir.join("zvec-bundled");
        let stamp = prefix.join(".stamp");
        let stamp_value = local_wheel
            .as_ref()
            .map(|p| format!("local:{}", p.display()))
            .unwrap_or_else(|| wheel.sha256.to_string());

        if stamp.exists()
            && fs::read_to_string(&stamp).ok().as_deref() == Some(stamp_value.as_str())
        {
            return Some(prefix);
        }

        if prefix.exists() {
            let _ = fs::remove_dir_all(&prefix);
        }
        fs::create_dir_all(prefix.join("lib")).ok()?;
        fs::create_dir_all(prefix.join("include").join("zvec")).ok()?;

        let wheel_bytes = if let Some(path) = local_wheel.as_ref() {
            fs::read(path).unwrap_or_else(|e| {
                panic!("bundled feature: failed to read {}: {e}", path.display())
            })
        } else {
            let bytes = fetch(wheel.url).unwrap_or_else(|e| {
                panic!("bundled feature: failed to download {}: {e}", wheel.url)
            });
            verify_sha256(&bytes, wheel.sha256);
            bytes
        };

        extract(&wheel_bytes, &prefix)
            .unwrap_or_else(|e| panic!("bundled feature: failed to unpack wheel: {e}"));

        let mut f = File::create(&stamp).ok()?;
        f.write_all(stamp_value.as_bytes()).ok()?;

        Some(prefix)
    }

    fn fetch(url: &str) -> Result<Vec<u8>, String> {
        let resp = ureq::get(url)
            .timeout(std::time::Duration::from_secs(300))
            .call()
            .map_err(|e| format!("{e}"))?;
        let mut buf = Vec::with_capacity(64 * 1024 * 1024);
        resp.into_reader()
            .read_to_end(&mut buf)
            .map_err(|e| format!("{e}"))?;
        Ok(buf)
    }

    fn verify_sha256(bytes: &[u8], expected: &str) {
        let got = format!("{:x}", Sha256::digest(bytes));
        if !got.eq_ignore_ascii_case(expected) {
            panic!("bundled feature: sha256 mismatch, wanted {expected}, got {got}");
        }
    }

    fn extract(wheel_bytes: &[u8], prefix: &Path) -> io::Result<()> {
        use std::io::Cursor;
        let cursor = Cursor::new(wheel_bytes);
        let mut archive = zip::ZipArchive::new(cursor)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

        let mut found_lib = false;
        let mut found_hdr = false;

        for i in 0..archive.len() {
            let mut entry = archive
                .by_index(i)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
            if entry.is_dir() {
                continue;
            }
            let name = entry.name().to_string();
            // Wheel layout shipped by upstream:
            //   lib/libzvec_c_api.{so,dylib}  or  lib/zvec_c_api.dll
            //   include/include/zvec/c_api.h  (note the double `include/`)
            let (dest_rel, track) = if let Some(stripped) = strip_lib_prefix(&name) {
                (prefix.join("lib").join(stripped), Track::Lib)
            } else if let Some(stripped) = strip_header_prefix(&name) {
                (
                    prefix.join("include").join("zvec").join(stripped),
                    Track::Hdr,
                )
            } else {
                continue;
            };

            if let Some(parent) = dest_rel.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut out = File::create(&dest_rel)?;
            io::copy(&mut entry, &mut out)?;

            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if matches!(track, Track::Lib) {
                    let _ = fs::set_permissions(&dest_rel, fs::Permissions::from_mode(0o755));
                }
            }

            match track {
                Track::Lib => found_lib = true,
                Track::Hdr => {
                    if dest_rel.file_name() == Some(std::ffi::OsStr::new("c_api.h")) {
                        found_hdr = true;
                    }
                }
            }
        }

        if !found_lib {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "wheel did not contain a libzvec_c_api library",
            ));
        }
        if !found_hdr {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "wheel did not contain include/zvec/c_api.h",
            ));
        }
        Ok(())
    }

    enum Track {
        Lib,
        Hdr,
    }

    fn strip_lib_prefix(name: &str) -> Option<&str> {
        // Only pick libzvec_c_api.so/.dylib or zvec_c_api.dll from the
        // wheel's top-level lib/ dir — everything else (the static archives,
        // Python extension) is uninteresting.
        let rest = name.strip_prefix("lib/")?;
        if rest.starts_with("libzvec_c_api.") || rest.eq_ignore_ascii_case("zvec_c_api.dll") {
            Some(rest)
        } else {
            None
        }
    }

    fn strip_header_prefix(name: &str) -> Option<&str> {
        // Upstream ships headers at `include/include/zvec/*` — pick the
        // public subset that starts with `zvec/`.
        name.strip_prefix("include/include/zvec/")
    }
}
