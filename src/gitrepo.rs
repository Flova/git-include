//! The `.gitrepo` marker file.
//!
//! git-include writes marker files that are byte-level compatible with
//! git-subrepo (https://github.com/ingydotnet/git-subrepo): an INI file
//! with a single `[subrepo]` section and tab-indented `key = value` lines.
//! A directory vendored with git-include can therefore be operated on with
//! git-subrepo and vice versa.

use std::path::Path;

use anyhow::{Context, Result, bail};

pub const MARKER_FILE: &str = ".gitrepo";

/// Parsed contents of a `.gitrepo` marker file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitRepoFile {
    /// URL (or remote name) of the upstream repository.
    pub remote: String,
    /// Upstream branch being tracked.
    pub branch: String,
    /// Upstream commit the subdirectory was last synced to.
    pub commit: String,
    /// Commit in the *host* repository that was HEAD when the last
    /// git-include/git-subrepo commit for this directory was made.
    pub parent: Option<String>,
    /// Sync method (git-subrepo supports "merge" and "rebase"; git-include
    /// always uses "merge" semantics but preserves the field when reading).
    pub method: String,
    /// Version of the tool that last wrote the file (informational).
    pub cmdver: String,
}

impl GitRepoFile {
    pub fn new(remote: &str, branch: &str, commit: &str, parent: Option<&str>) -> Self {
        GitRepoFile {
            remote: remote.to_string(),
            branch: branch.to_string(),
            commit: commit.to_string(),
            parent: parent.map(str::to_string),
            method: "merge".to_string(),
            cmdver: env!("CARGO_PKG_VERSION").to_string(),
        }
    }

    /// Parse the marker file format. Accepts anything git-subrepo writes
    /// (and is lenient about whitespace so hand-edited files still parse).
    pub fn parse(input: &str) -> Result<Self> {
        let mut in_section = false;
        let mut remote = None;
        let mut branch = None;
        let mut commit = None;
        let mut parent = None;
        let mut method = None;
        let mut cmdver = None;

        for line in input.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
                continue;
            }
            if line.starts_with('[') {
                in_section = line == "[subrepo]";
                continue;
            }
            if !in_section {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let key = key.trim();
            let value = value.trim().to_string();
            match key {
                "remote" => remote = Some(value),
                "branch" => branch = Some(value),
                "commit" => commit = Some(value),
                "parent" => parent = Some(value),
                "method" => method = Some(value),
                "cmdver" => cmdver = Some(value),
                _ => {} // ignore unknown keys for forward compatibility
            }
        }

        Ok(GitRepoFile {
            remote: remote.context("marker file has no 'remote' key in [subrepo] section")?,
            branch: branch.context("marker file has no 'branch' key in [subrepo] section")?,
            commit: commit.context("marker file has no 'commit' key in [subrepo] section")?,
            parent: parent.filter(|p| !p.is_empty() && p != "none"),
            method: method.unwrap_or_else(|| "merge".to_string()),
            cmdver: cmdver.unwrap_or_default(),
        })
    }

    pub fn load(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("cannot read marker file {}", path.display()))?;
        Self::parse(&content)
            .with_context(|| format!("cannot parse marker file {}", path.display()))
    }

    /// Serialize in git-subrepo's exact layout (tab indentation, same key
    /// order, same comment header style).
    pub fn serialize(&self) -> String {
        let mut out = String::new();
        out.push_str(
            "; DO NOT EDIT (unless you know what you are doing)\n\
             ;\n\
             ; This subdirectory is a git \"subrepo\", and this file is maintained by the\n\
             ; git-include command (compatible with git-subrepo).\n\
             ; See https://github.com/flova/git-include\n\
             ;\n",
        );
        out.push_str("[subrepo]\n");
        out.push_str(&format!("\tremote = {}\n", self.remote));
        out.push_str(&format!("\tbranch = {}\n", self.branch));
        out.push_str(&format!("\tcommit = {}\n", self.commit));
        if let Some(parent) = &self.parent {
            out.push_str(&format!("\tparent = {parent}\n"));
        }
        out.push_str(&format!("\tmethod = {}\n", self.method));
        out.push_str(&format!("\tcmdver = {}\n", self.cmdver));
        out
    }
}

/// Validate that a subdirectory string is safe to use as an include prefix.
pub fn validate_subdir(subdir: &str) -> Result<()> {
    if subdir.is_empty() || subdir == "." {
        bail!("subdirectory must not be the repository root");
    }
    if subdir.starts_with('/') || subdir.contains("..") {
        bail!("subdirectory must be a relative path inside the repository (got '{subdir}')");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let f = GitRepoFile::new(
            "https://example.com/repo.git",
            "main",
            "0123456789abcdef0123456789abcdef01234567",
            Some("fedcba9876543210fedcba9876543210fedcba98"),
        );
        let parsed = GitRepoFile::parse(&f.serialize()).unwrap();
        assert_eq!(f, parsed);
    }

    #[test]
    fn parses_git_subrepo_output() {
        // Verbatim output of git-subrepo 0.4.9.
        let input = "; DO NOT EDIT (unless you know what you are doing)\n\
             ;\n\
             ; This subdirectory is a git \"subrepo\", and this file is maintained by the\n\
             ; git-subrepo command. See https://github.com/ingydotnet/git-subrepo#readme\n\
             ;\n\
             [subrepo]\n\
             \tremote = https://github.com/user/lib.git\n\
             \tbranch = master\n\
             \tcommit = 2f3b8a9c1d0e5f6a7b8c9d0e1f2a3b4c5d6e7f8a\n\
             \tparent = 9a8b7c6d5e4f3a2b1c0d9e8f7a6b5c4d3e2f1a0b\n\
             \tmethod = merge\n\
             \tcmdver = 0.4.9\n";
        let f = GitRepoFile::parse(input).unwrap();
        assert_eq!(f.remote, "https://github.com/user/lib.git");
        assert_eq!(f.branch, "master");
        assert_eq!(f.commit, "2f3b8a9c1d0e5f6a7b8c9d0e1f2a3b4c5d6e7f8a");
        assert_eq!(
            f.parent.as_deref(),
            Some("9a8b7c6d5e4f3a2b1c0d9e8f7a6b5c4d3e2f1a0b")
        );
        assert_eq!(f.method, "merge");
        assert_eq!(f.cmdver, "0.4.9");
    }

    #[test]
    fn lenient_parsing() {
        // Spaces instead of tabs, missing optional keys, extra unknown keys.
        let input = "[subrepo]\n  remote=x\n  branch=dev\n  commit=abc\n  future-key = 1\n";
        let f = GitRepoFile::parse(input).unwrap();
        assert_eq!(f.remote, "x");
        assert_eq!(f.branch, "dev");
        assert_eq!(f.commit, "abc");
        assert_eq!(f.parent, None);
        assert_eq!(f.method, "merge");
    }

    #[test]
    fn rejects_bad_subdirs() {
        assert!(validate_subdir("").is_err());
        assert!(validate_subdir(".").is_err());
        assert!(validate_subdir("/abs").is_err());
        assert!(validate_subdir("a/../b").is_err());
        assert!(validate_subdir("vendor/lib").is_ok());
    }
}
