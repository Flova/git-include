//! `git include self-update`: replace the running binary with a release
//! from GitHub.
//!
//! Downloads go through `curl` (the same transport the install script
//! uses), so no HTTP stack is compiled into the binary. Release assets are
//! plain executables named `git-include-<target-triple>[.exe]`, verified
//! against the release's SHA256SUMS file before installation.

#[cfg(feature = "self-update")]
mod imp {
    use std::path::Path;
    use std::process::Command;

    use anyhow::{Context, Result, bail};

    const REPO: &str = "flova/git-include";

    pub fn run(version: Option<&str>, dry_run: bool) -> Result<()> {
        let current = env!("CARGO_PKG_VERSION");
        let target = target_triple()
            .context("self-update is not supported on this platform; update via your installer")?;

        let tag = match version {
            Some(v) => {
                let v = v.trim_start_matches('v');
                format!("v{v}")
            }
            None => latest_release_tag()?,
        };
        let latest = tag.trim_start_matches('v');

        if latest == current {
            println!("git-include {current} is already the latest version.");
            return Ok(());
        }
        println!("Updating git-include {current} -> {latest} ...");
        if dry_run {
            println!("dry run: would download and install release {tag}.");
            return Ok(());
        }

        let suffix = if cfg!(windows) { ".exe" } else { "" };
        let asset = format!("git-include-{target}{suffix}");
        let url = format!("https://github.com/{REPO}/releases/download/{tag}/{asset}");

        let exe = std::env::current_exe().context("cannot locate the running executable")?;
        // Download next to the current binary so the final rename stays on one
        // filesystem (and is atomic on Unix).
        let staging = exe.with_extension("update-tmp");
        let _ = std::fs::remove_file(&staging);
        curl_to_file(&url, &staging).with_context(|| format!("could not download {url}"))?;

        // Verify against the release's checksum manifest before touching
        // the installed binary.
        let sums_url = format!("https://github.com/{REPO}/releases/download/{tag}/SHA256SUMS");
        let sums = curl_stdout(&sums_url)
            .context("could not download SHA256SUMS to verify the release")?;
        if let Err(err) = verify_sha256(&staging, &sums, &asset) {
            let _ = std::fs::remove_file(&staging);
            return Err(err);
        }

        replace_executable(&exe, &staging)?;
        println!("Updated {} to git-include {latest}.", exe.display());
        Ok(())
    }

    /// Resolve the latest release tag via the GitHub API.
    fn latest_release_tag() -> Result<String> {
        let body = curl_stdout(&format!(
            "https://api.github.com/repos/{REPO}/releases/latest"
        ))
        .context("could not query GitHub for the latest release")?;
        extract_json_string(&body, "tag_name")
            .context("unexpected GitHub API response (no tag_name found)")
    }

    /// Minimal extraction of a top-level `"key": "value"` pair from a JSON
    /// document — enough for the releases API, without a JSON dependency.
    fn extract_json_string(json: &str, key: &str) -> Option<String> {
        let needle = format!("\"{key}\"");
        let start = json.find(&needle)? + needle.len();
        let rest = json[start..].trim_start().strip_prefix(':')?.trim_start();
        let rest = rest.strip_prefix('"')?;
        Some(rest[..rest.find('"')?].to_string())
    }

    fn curl(args: &[&str]) -> Result<std::process::Output> {
        let out = Command::new("curl")
            .args(["-fsSL", "--retry", "3"])
            .args(args)
            .output()
            .context("curl is required for self-update but was not found on PATH")?;
        if !out.status.success() {
            bail!(
                "curl failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        Ok(out)
    }

    fn curl_stdout(url: &str) -> Result<String> {
        Ok(String::from_utf8_lossy(&curl(&[url])?.stdout).into_owned())
    }

    fn curl_to_file(url: &str, path: &Path) -> Result<()> {
        curl(&[url, "-o", path.to_str().context("non-UTF-8 path")?])?;
        Ok(())
    }

    /// Look up `asset` in a `sha256sum`-formatted manifest.
    fn expected_hash<'a>(sums: &'a str, asset: &str) -> Option<&'a str> {
        sums.lines().find_map(|line| {
            let (hash, name) = line.split_once(char::is_whitespace)?;
            let name = name.trim().trim_start_matches('*');
            (name == asset).then_some(hash)
        })
    }

    fn verify_sha256(path: &Path, sums: &str, asset: &str) -> Result<()> {
        use sha2::{Digest, Sha256};
        let expected = expected_hash(sums, asset)
            .with_context(|| format!("SHA256SUMS has no entry for {asset}"))?;
        let data = std::fs::read(path)?;
        let actual: String = Sha256::digest(&data)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        if !actual.eq_ignore_ascii_case(expected) {
            bail!(
                "checksum mismatch for {asset}: expected {expected}, got {actual}.\n\
                 The download was discarded; try again or verify the release manually."
            );
        }
        Ok(())
    }

    /// Swap the new binary into place. On Unix a rename over the running
    /// executable is fine; on Windows the running image must be renamed away
    /// first.
    fn replace_executable(exe: &Path, staging: &Path) -> Result<()> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(staging, std::fs::Permissions::from_mode(0o755))?;
            std::fs::rename(staging, exe).context("could not replace the executable")?;
        }
        #[cfg(windows)]
        {
            let old = exe.with_extension("old.exe");
            let _ = std::fs::remove_file(&old);
            std::fs::rename(exe, &old).context("could not move the running executable aside")?;
            if let Err(e) = std::fs::rename(staging, exe) {
                let _ = std::fs::rename(&old, exe); // roll back
                return Err(e).context("could not install the new executable");
            }
        }
        Ok(())
    }

    /// The release asset suffix for this build. Linux ships two flavors —
    /// dynamic (system OpenSSL) and portable (everything compiled in,
    /// built with GIT_INCLUDE_PORTABLE=1) — and each updates to its own
    /// kind.
    fn target_triple() -> Option<&'static str> {
        let portable = option_env!("GIT_INCLUDE_PORTABLE").is_some();
        Some(match (std::env::consts::OS, std::env::consts::ARCH) {
            ("linux", "x86_64") if portable => "x86_64-unknown-linux-gnu-portable",
            ("linux", "x86_64") => "x86_64-unknown-linux-gnu",
            ("linux", "aarch64") if portable => "aarch64-unknown-linux-gnu-portable",
            ("linux", "aarch64") => "aarch64-unknown-linux-gnu",
            // No x86_64 macOS asset is published (Intel runners are too
            // scarce in CI); those builds come from cargo/conda instead.
            ("macos", "aarch64") => "aarch64-apple-darwin",
            ("windows", "x86_64") => "x86_64-pc-windows-msvc",
            ("windows", "aarch64") => "aarch64-pc-windows-msvc",
            _ => return None,
        })
    }

    #[cfg(test)]
    mod tests {
        use super::{expected_hash, extract_json_string};

        #[test]
        fn finds_asset_hashes_in_sha256sums() {
            let sums = "aaaa  git-include-x86_64-unknown-linux-gnu\n\
                        bbbb *git-include-x86_64-pc-windows-msvc.exe\n";
            assert_eq!(
                expected_hash(sums, "git-include-x86_64-unknown-linux-gnu"),
                Some("aaaa")
            );
            assert_eq!(
                expected_hash(sums, "git-include-x86_64-pc-windows-msvc.exe"),
                Some("bbbb")
            );
            assert_eq!(expected_hash(sums, "other"), None);
        }

        #[test]
        fn extracts_tag_name_from_release_json() {
            let json = r#"{"url": "x", "tag_name": "v0.2.0", "name": "git-include 0.2.0"}"#;
            assert_eq!(
                extract_json_string(json, "tag_name").as_deref(),
                Some("v0.2.0")
            );
        }

        #[test]
        fn tolerates_whitespace_and_missing_keys() {
            assert_eq!(
                extract_json_string("{ \"tag_name\" :  \"v1\" }", "tag_name").as_deref(),
                Some("v1")
            );
            assert_eq!(extract_json_string("{}", "tag_name"), None);
        }
    }
}

#[cfg(feature = "self-update")]
pub use imp::run;

/// Package-manager builds (e.g. conda) compile without the `self-update`
/// feature: the binary must not replace itself behind the manager's back.
#[cfg(not(feature = "self-update"))]
pub fn run(_version: Option<&str>, _dry_run: bool) -> anyhow::Result<()> {
    anyhow::bail!(
        "this git-include build was installed through a package manager, so \
         self-update is disabled.\nUpdate it the same way it was installed \
         (e.g. `conda update git-include`)."
    )
}
