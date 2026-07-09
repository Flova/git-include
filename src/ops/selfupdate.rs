//! `git include self-update`: replace the running binary with a release
//! from GitHub.
//!
//! Downloads go through `curl` (the same transport the install script
//! uses), so no HTTP stack is compiled into the binary. Release assets are
//! plain executables named `git-include-<target-triple>[.exe]`.

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

    /// The release target triple for this build.
    fn target_triple() -> Option<&'static str> {
        Some(match (std::env::consts::OS, std::env::consts::ARCH) {
            ("linux", "x86_64") => "x86_64-unknown-linux-gnu",
            ("linux", "aarch64") => "aarch64-unknown-linux-gnu",
            ("macos", "x86_64") => "x86_64-apple-darwin",
            ("macos", "aarch64") => "aarch64-apple-darwin",
            ("windows", "x86_64") => "x86_64-pc-windows-msvc",
            _ => return None,
        })
    }

    #[cfg(test)]
    mod tests {
        use super::extract_json_string;

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
