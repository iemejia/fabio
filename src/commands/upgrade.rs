use std::fmt::Write;
use std::io::{Cursor, Read};

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::cli::Cli;
use crate::output;

const GITHUB_REPO: &str = "iemejia/fabio";
const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Deserialize)]
struct Release {
    tag_name: String,
}

/// Execute the upgrade command.
pub async fn execute(cli: &Cli, check: bool, version: Option<&str>, force: bool) -> Result<()> {
    let target_version = if let Some(v) = version {
        // Strip leading 'v' if provided
        v.strip_prefix('v').unwrap_or(v).to_string()
    } else {
        fetch_latest_version().await?
    };

    let is_current = target_version == CURRENT_VERSION;
    let is_newer = is_version_newer(&target_version, CURRENT_VERSION);

    if check {
        // Warm the version-check cache when we fetched the real latest release
        // (not an explicit --target-version). This makes both a manual check and
        // the background refresher populate the same cache consumed by the
        // passive "newer fabio available" notice.
        if version.is_none() {
            crate::version_check::write_cache(&target_version);
        }
        let obj = serde_json::json!({
            "current_version": CURRENT_VERSION,
            "latest_version": target_version,
            "update_available": is_newer,
        });
        output::render_object(cli, &obj, "current_version");
        return Ok(());
    }

    // Refuse to upgrade development builds unless --force is used
    if is_dev_build() && !force {
        let obj = serde_json::json!({
            "status": "dev_build",
            "current_version": CURRENT_VERSION,
            "message": format!(
                "You are running a development build ({CURRENT_VERSION}). \
                 upgrade only updates official released versions. \
                 Use --force to override, or install a release with: \
                 cargo install --git https://github.com/{GITHUB_REPO}.git --tag <version>"
            ),
        });
        output::render_object(cli, &obj, "status");
        return Ok(());
    }

    if is_current && !force {
        let obj = serde_json::json!({
            "status": "up_to_date",
            "current_version": CURRENT_VERSION,
            "message": format!("Already running the latest version ({CURRENT_VERSION})."),
        });
        output::render_object(cli, &obj, "status");
        return Ok(());
    }

    // Refuse to downgrade unless --force is used
    if !is_newer && !force {
        let obj = serde_json::json!({
            "status": "up_to_date",
            "current_version": CURRENT_VERSION,
            "latest_version": target_version,
            "message": format!(
                "Current version ({CURRENT_VERSION}) is newer than the latest release ({target_version}). Use --force to downgrade."
            ),
        });
        output::render_object(cli, &obj, "status");
        return Ok(());
    }

    // Dry-run guard
    if output::dry_run_guard(
        cli,
        &format!("upgrade from {CURRENT_VERSION} to {target_version}"),
        &serde_json::json!({
            "current_version": CURRENT_VERSION,
            "target_version": target_version,
            "artifact": artifact_name(),
        }),
    ) {
        return Ok(());
    }

    download_and_install(&target_version).await?;

    let obj = serde_json::json!({
        "status": "updated",
        "previous_version": CURRENT_VERSION,
        "new_version": target_version,
        "message": format!("Successfully updated from {CURRENT_VERSION} to {target_version}."),
    });
    output::render_object(cli, &obj, "status");
    Ok(())
}

/// Download, verify, extract, and install the target version.
async fn download_and_install(target_version: &str) -> Result<()> {
    let artifact = artifact_name();
    let tag = format!("v{target_version}");
    let archive_url =
        format!("https://github.com/{GITHUB_REPO}/releases/download/{tag}/{artifact}");
    let checksum_url = format!("{archive_url}.sha256");

    eprintln!("[upgrade] Downloading {artifact} ({target_version})...");

    // Download archive and checksum
    let http = crate::client::http_client_builder()
        .user_agent(format!("fabio/{CURRENT_VERSION}"))
        .build()?;

    let archive_bytes = http
        .get(&archive_url)
        .send()
        .await?
        .error_for_status()
        .with_context(|| format!("Failed to download {archive_url}"))?
        .bytes()
        .await?;

    let checksum_text = http
        .get(&checksum_url)
        .send()
        .await?
        .error_for_status()
        .with_context(|| format!("Failed to download checksum from {checksum_url}"))?
        .text()
        .await?;

    // Verify SHA256
    eprintln!("[upgrade] Verifying checksum...");
    verify_checksum(&archive_bytes, &checksum_text)?;

    // Extract binary from archive
    eprintln!("[upgrade] Extracting binary...");
    let binary = extract_binary(&archive_bytes)?;

    // Replace current executable
    eprintln!("[upgrade] Replacing binary...");
    replace_executable(&binary)?;

    Ok(())
}

/// Fetch the latest release version from GitHub.
async fn fetch_latest_version() -> Result<String> {
    let url = format!("https://api.github.com/repos/{GITHUB_REPO}/releases/latest");
    let http = crate::client::http_client_builder()
        .user_agent(format!("fabio/{CURRENT_VERSION}"))
        .build()?;

    let release: Release = http
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await?
        .error_for_status()
        .with_context(|| "Failed to fetch latest release from GitHub")?
        .json()
        .await?;

    let version = release
        .tag_name
        .strip_prefix('v')
        .unwrap_or(&release.tag_name)
        .to_string();
    Ok(version)
}

/// Determine the artifact name for the current platform.
const fn artifact_name() -> &'static str {
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    {
        "fabio-linux-x64.tar.gz"
    }
    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    {
        "fabio-linux-arm64.tar.gz"
    }
    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    {
        "fabio-macos-x64.tar.gz"
    }
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    {
        "fabio-macos-arm64.tar.gz"
    }
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    {
        "fabio-windows-x64.zip"
    }
    #[cfg(all(target_os = "windows", target_arch = "aarch64"))]
    {
        "fabio-windows-arm64.zip"
    }
    #[cfg(not(any(
        all(target_os = "linux", target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "aarch64"),
        all(target_os = "macos", target_arch = "x86_64"),
        all(target_os = "macos", target_arch = "aarch64"),
        all(target_os = "windows", target_arch = "x86_64"),
        all(target_os = "windows", target_arch = "aarch64"),
    )))]
    {
        compile_error!("Unsupported platform for upgrade");
    }
}

/// Returns true if the current binary is a development build (version contains `-dev`).
const fn is_dev_build() -> bool {
    // const-compatible: check for '-' in version string (release versions are pure x.y.z)
    let bytes = CURRENT_VERSION.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'-' {
            return true;
        }
        i += 1;
    }
    false
}

/// Compare two version strings (major.minor.patch) and return true if `target` is newer than `current`.
pub fn is_version_newer(target: &str, current: &str) -> bool {
    let parse = |v: &str| -> (u32, u32, u32) {
        let mut parts = v.split('.').map(|p| p.parse::<u32>().unwrap_or(0));
        let major = parts.next().unwrap_or(0);
        let minor = parts.next().unwrap_or(0);
        let patch = parts.next().unwrap_or(0);
        (major, minor, patch)
    };
    parse(target) > parse(current)
}

/// Verify the SHA256 checksum of the downloaded archive.
fn verify_checksum(data: &[u8], checksum_text: &str) -> Result<()> {
    let expected = checksum_text
        .split_whitespace()
        .next()
        .context("Empty checksum file")?
        .to_lowercase();

    let mut hasher = Sha256::new();
    hasher.update(data);
    let hash_bytes = hasher.finalize();
    let actual = hash_bytes
        .iter()
        .fold(String::with_capacity(64), |mut s, b| {
            let _ = write!(s, "{b:02x}");
            s
        });

    if actual != expected {
        bail!(
            "Checksum mismatch: expected {expected}, got {actual}. \
             The download may be corrupted."
        );
    }
    Ok(())
}

/// Extract the fabio binary from the downloaded archive.
#[cfg(not(windows))]
fn extract_binary(archive: &[u8]) -> Result<Vec<u8>> {
    use flate2::read::GzDecoder;

    let decoder = GzDecoder::new(Cursor::new(archive));
    let mut tar = tar::Archive::new(decoder);

    for entry in tar.entries()? {
        let mut entry = entry?;
        let path = entry.path()?;
        if path.file_name().and_then(|n| n.to_str()) == Some("fabio") {
            let mut buf = Vec::new();
            entry.read_to_end(&mut buf)?;
            return Ok(buf);
        }
    }
    bail!("Binary 'fabio' not found in archive");
}

/// Extract the fabio binary from the downloaded archive (Windows zip).
#[cfg(windows)]
fn extract_binary(archive: &[u8]) -> Result<Vec<u8>> {
    let reader = Cursor::new(archive);
    let mut zip = zip::ZipArchive::new(reader)?;

    for i in 0..zip.len() {
        let mut file = zip.by_index(i)?;
        let name = file.name().to_string();
        if name == "fabio.exe" || name.ends_with("/fabio.exe") {
            let mut buf = Vec::new();
            file.read_to_end(&mut buf)?;
            return Ok(buf);
        }
    }
    bail!("Binary 'fabio.exe' not found in archive");
}

/// Replace the currently running executable with the new binary.
#[cfg(not(windows))]
fn replace_executable(new_binary: &[u8]) -> Result<()> {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    let current_exe =
        std::env::current_exe().context("Cannot determine current executable path")?;
    let current_exe = current_exe
        .canonicalize()
        .unwrap_or_else(|_| current_exe.clone());

    // Write to a temp file in the same directory (ensures same filesystem for atomic rename)
    let parent = current_exe
        .parent()
        .context("Cannot determine parent directory of current executable")?;
    let tmp_path = parent.join(".fabio-update.tmp");

    fs::write(&tmp_path, new_binary)
        .with_context(|| format!("Failed to write temporary file: {}", tmp_path.display()))?;

    // Set executable permissions (rwxr-xr-x)
    fs::set_permissions(&tmp_path, fs::Permissions::from_mode(0o755))?;

    // Atomic rename
    fs::rename(&tmp_path, &current_exe).with_context(|| {
        format!(
            "Failed to replace executable at {}. You may need to run with elevated permissions.",
            current_exe.display()
        )
    })?;

    Ok(())
}

/// Replace the currently running executable with the new binary (Windows).
/// On Windows, the running exe is locked, so we rename it first.
#[cfg(windows)]
fn replace_executable(new_binary: &[u8]) -> Result<()> {
    use std::fs;

    let current_exe =
        std::env::current_exe().context("Cannot determine current executable path")?;
    let current_exe = current_exe
        .canonicalize()
        .unwrap_or_else(|_| current_exe.clone());

    let parent = current_exe
        .parent()
        .context("Cannot determine parent directory of current executable")?;
    let old_path = parent.join("fabio.exe.old");
    let tmp_path = parent.join(".fabio-update.tmp");

    // Write new binary to temp file
    fs::write(&tmp_path, new_binary)
        .with_context(|| format!("Failed to write temporary file: {}", tmp_path.display()))?;

    // Remove any leftover .old file from a previous update
    let _ = fs::remove_file(&old_path);

    // Rename current exe out of the way (Windows allows renaming a running exe)
    fs::rename(&current_exe, &old_path).with_context(|| {
        "Failed to rename current executable. You may need to run with elevated permissions."
            .to_string()
    })?;

    // Move new binary into place
    if let Err(e) = fs::rename(&tmp_path, &current_exe) {
        // Rollback: restore the original
        let _ = fs::rename(&old_path, &current_exe);
        return Err(e).with_context(|| "Failed to install new binary");
    }

    // Best-effort cleanup of old binary
    let _ = fs::remove_file(&old_path);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[allow(clippy::case_sensitive_file_extension_comparisons)]
    fn test_artifact_name_is_known() {
        let name = artifact_name();
        assert!(
            name.starts_with("fabio-"),
            "Unexpected artifact name: {name}"
        );
        assert!(
            name.ends_with(".tar.gz") || name.ends_with(".zip"),
            "Unexpected archive format: {name}"
        );
    }

    #[test]
    fn test_verify_checksum_valid() {
        let data = b"hello world";
        let hash_bytes = Sha256::digest(data);
        let mut hash = String::with_capacity(64);
        for b in &hash_bytes {
            write!(hash, "{b:02x}").unwrap();
        }
        let checksum_text = format!("{hash}  fabio-linux-x64.tar.gz\n");
        assert!(verify_checksum(data, &checksum_text).is_ok());
    }

    #[test]
    fn test_verify_checksum_invalid() {
        let data = b"hello world";
        let checksum_text = "0000000000000000000000000000000000000000000000000000000000000000  fabio-linux-x64.tar.gz\n";
        assert!(verify_checksum(data, checksum_text).is_err());
    }

    #[test]
    fn test_verify_checksum_empty() {
        let data = b"hello world";
        assert!(verify_checksum(data, "").is_err());
    }

    #[test]
    fn test_current_version_is_valid() {
        // Ensure version string is a valid semver-like format (x.y.z or x.y.z-prerelease)
        let base = CURRENT_VERSION.split('-').next().unwrap();
        let parts: Vec<&str> = base.split('.').collect();
        assert_eq!(parts.len(), 3, "Version should be major.minor.patch");
        for part in parts {
            assert!(
                part.parse::<u32>().is_ok(),
                "Version component '{part}' is not a number"
            );
        }
    }

    #[test]
    fn test_is_dev_build_detection() {
        // Verify is_dev_build() correctly detects the -dev suffix
        let has_dev = CURRENT_VERSION.contains('-');
        assert_eq!(is_dev_build(), has_dev);
    }

    #[test]
    fn test_is_version_newer_basic() {
        assert!(is_version_newer("1.0.0", "0.9.9"));
        assert!(is_version_newer("0.2.0", "0.1.0"));
        assert!(is_version_newer("0.1.1", "0.1.0"));
        assert!(is_version_newer("0.24.0", "0.1.0"));
    }

    #[test]
    fn test_is_version_newer_same() {
        assert!(!is_version_newer("0.24.0", "0.24.0"));
        assert!(!is_version_newer("1.0.0", "1.0.0"));
    }

    #[test]
    fn test_is_version_newer_older() {
        assert!(!is_version_newer("0.1.0", "0.24.0"));
        assert!(!is_version_newer("0.23.9", "0.24.0"));
        assert!(!is_version_newer("0.0.1", "1.0.0"));
    }

    #[test]
    fn test_is_version_newer_major_trumps_minor() {
        assert!(is_version_newer("2.0.0", "1.99.99"));
        assert!(!is_version_newer("1.99.99", "2.0.0"));
    }

    #[cfg(not(windows))]
    #[test]
    fn test_extract_binary_from_tar_gz() {
        use flate2::Compression;
        use flate2::write::GzEncoder;

        // Create a tar.gz with a single "fabio" file
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        {
            let mut builder = tar::Builder::new(&mut encoder);
            let content = b"#!/bin/sh\necho hello";
            let mut header = tar::Header::new_gnu();
            header.set_size(content.len() as u64);
            header.set_mode(0o755);
            header.set_cksum();
            builder
                .append_data(&mut header, "fabio", &content[..])
                .unwrap();
            builder.finish().unwrap();
        }
        let archive = encoder.finish().unwrap();

        let binary = extract_binary(&archive).unwrap();
        assert_eq!(binary, b"#!/bin/sh\necho hello");
    }

    #[cfg(not(windows))]
    #[test]
    fn test_extract_binary_missing() {
        use flate2::Compression;
        use flate2::write::GzEncoder;

        // Create a tar.gz with a different filename
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        {
            let mut builder = tar::Builder::new(&mut encoder);
            let content = b"data";
            let mut header = tar::Header::new_gnu();
            header.set_size(content.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            builder
                .append_data(&mut header, "not-fabio", &content[..])
                .unwrap();
            builder.finish().unwrap();
        }
        let archive = encoder.finish().unwrap();

        assert!(extract_binary(&archive).is_err());
    }
}
