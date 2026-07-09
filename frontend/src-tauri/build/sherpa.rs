// Build-time staging of the sherpa-onnx shared DLLs (speaker diarization).
//
// The `sherpa-onnx-sys` crate links against shared DLLs on Windows (the static
// prebuilt archives are /MT and clash with this app's /MD runtime) and copies
// them into target/{debug,release} for development runs. Installers, however,
// only include what tauri.conf.json declares — and tauri-build validates those
// resource paths BEFORE the dependency's build script is guaranteed to have
// run. So this module stages the DLLs at a stable path (binaries/sherpa/) that
// tauri.windows.conf.json references, downloading the prebuilt archive into
// the same cache location sherpa-onnx-sys uses if it isn't there yet.

/// Keep in sync with the `sherpa-onnx` version pinned in Cargo.toml
const SHERPA_ONNX_VERSION: &str = "1.13.4";

pub fn ensure_sherpa_dlls() {
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os != "windows" {
        return;
    }

    if let Err(e) = stage_dlls() {
        // Fail the build with a clear message: without the DLLs the bundled
        // app would not start (sherpa-onnx-c-api.dll is a load-time import).
        panic!("Failed to stage sherpa-onnx DLLs for bundling: {}", e);
    }
}

fn stage_dlls() -> Result<(), String> {
    let manifest_dir = std::path::PathBuf::from(
        std::env::var("CARGO_MANIFEST_DIR").map_err(|e| e.to_string())?,
    );
    let staging_dir = manifest_dir.join("binaries").join("sherpa");

    let lib_dir = ensure_prebuilt_archive()?;

    std::fs::create_dir_all(&staging_dir)
        .map_err(|e| format!("Failed to create {}: {}", staging_dir.display(), e))?;

    let mut copied = 0usize;
    for entry in std::fs::read_dir(&lib_dir)
        .map_err(|e| format!("Failed to read {}: {}", lib_dir.display(), e))?
    {
        let path = entry.map_err(|e| e.to_string())?.path();
        if path.extension().and_then(|e| e.to_str()) == Some("dll") {
            let dest = staging_dir.join(path.file_name().unwrap());
            std::fs::copy(&path, &dest)
                .map_err(|e| format!("Failed to copy {}: {}", path.display(), e))?;
            copied += 1;
        }
    }

    if copied == 0 {
        return Err(format!("No DLLs found in {}", lib_dir.display()));
    }

    println!(
        "cargo:warning=✅ Staged {} sherpa-onnx DLLs into binaries/sherpa/",
        copied
    );
    Ok(())
}

/// Ensure the sherpa-onnx shared prebuilt archive is downloaded and extracted.
/// Uses the exact same cache location as the sherpa-onnx-sys build script
/// (<target>/sherpa-onnx-prebuilt/) so the archive is only fetched once.
fn ensure_prebuilt_archive() -> Result<std::path::PathBuf, String> {
    let archive_stem = format!(
        "sherpa-onnx-v{}-win-x64-shared-MT-Release-lib",
        SHERPA_ONNX_VERSION
    );
    let archive_name = format!("{}.tar.bz2", archive_stem);

    let cache_root = target_dir()?.join("sherpa-onnx-prebuilt");
    let lib_dir = cache_root.join(&archive_stem).join("lib");
    if lib_dir.is_dir() {
        return Ok(lib_dir);
    }

    std::fs::create_dir_all(&cache_root).map_err(|e| e.to_string())?;

    let archive_path = cache_root.join(&archive_name);
    if !archive_path.is_file() {
        let url = format!(
            "https://github.com/k2-fsa/sherpa-onnx/releases/download/v{}/{}",
            SHERPA_ONNX_VERSION, archive_name
        );
        println!("cargo:warning=⬇️  Downloading sherpa-onnx shared libs from {}", url);

        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(600))
            .build()
            .map_err(|e| format!("Failed to create HTTP client: {}", e))?;
        let response = client
            .get(&url)
            .send()
            .map_err(|e| format!("Failed to download {}: {}", url, e))?;
        if !response.status().is_success() {
            return Err(format!("HTTP error {} for {}", response.status(), url));
        }
        let content = response
            .bytes()
            .map_err(|e| format!("Failed to read download: {}", e))?;

        // Atomic write (temp + rename) so a concurrent sherpa-onnx-sys build
        // script never sees a partial archive
        let temp_path = cache_root.join(format!("{}.part2", archive_name));
        std::fs::write(&temp_path, &content).map_err(|e| e.to_string())?;
        if std::fs::rename(&temp_path, &archive_path).is_err() {
            // Another build script won the race - its copy is fine
            let _ = std::fs::remove_file(&temp_path);
        }
    }

    let tar_file = std::fs::File::open(&archive_path).map_err(|e| e.to_string())?;
    let decoder = bzip2::read::BzDecoder::new(tar_file);
    let mut archive = tar::Archive::new(decoder);
    archive
        .unpack(&cache_root)
        .map_err(|e| format!("Failed to extract {}: {}", archive_path.display(), e))?;

    if !lib_dir.is_dir() {
        return Err(format!(
            "Extracted archive has no lib dir: {}",
            lib_dir.display()
        ));
    }
    Ok(lib_dir)
}

/// Resolve the cargo target directory from OUT_DIR (or CARGO_TARGET_DIR)
fn target_dir() -> Result<std::path::PathBuf, String> {
    if let Ok(dir) = std::env::var("CARGO_TARGET_DIR") {
        return Ok(std::path::PathBuf::from(dir));
    }
    let out_dir = std::path::PathBuf::from(std::env::var("OUT_DIR").map_err(|e| e.to_string())?);
    out_dir
        .ancestors()
        .find(|p| p.file_name().map(|n| n == "target").unwrap_or(false))
        .map(|p| p.to_path_buf())
        .ok_or_else(|| format!("Could not locate target dir from {}", out_dir.display()))
}
