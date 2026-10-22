use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::git::Git;

/// Convert a user-supplied directory argument (relative to the current
/// working directory) into a repository-relative path with `/` separators
/// and no trailing slash. Works even if the directory does not exist yet.
pub fn repo_relative_subdir(git: &Git, arg: &Path) -> Result<String> {
    let cwd = std::env::current_dir().context("cannot determine current directory")?;
    let cwd = std::fs::canonicalize(&cwd).unwrap_or(cwd);
    let toplevel = std::fs::canonicalize(&git.toplevel).unwrap_or_else(|_| git.toplevel.clone());

    let abs = if arg.is_absolute() {
        arg.to_path_buf()
    } else {
        cwd.join(arg)
    };
    let abs = lexical_normalize(&abs);

    let rel = abs.strip_prefix(&toplevel).map_err(|_| {
        anyhow::anyhow!(
            "'{}' is outside the git repository at {}",
            arg.display(),
            toplevel.display()
        )
    })?;

    let rel = rel
        .to_str()
        .context("subdirectory path is not valid UTF-8")?
        .trim_end_matches('/')
        .replace('\\', "/");
    if rel.is_empty() {
        bail!("subdirectory must not be the repository root");
    }
    Ok(rel)
}

/// Purely lexical path normalization (resolves `.` and `..` without
/// touching the filesystem).
fn lexical_normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Prefix of the refs that pin fetched upstream commits (see [`pin_ref`]).
pub const PIN_REF_PREFIX: &str = "refs/include/upstream/";

/// Ref under which fetched upstream commits are pinned, so they survive
/// `git gc` and enable offline `status`/`diff`.
///
/// The ref holds "the latest known commit of `rev` on `remote`", so it is
/// keyed by exactly that, not by the include's directory: moving an
/// include keeps its pin, and a directory reused for another upstream (or
/// tracking another branch on another host branch) can never read a stale
/// pin. Includes tracking the same remote and rev share one ref, which is
/// correct since they share the upstream head.
///
/// The name is the repository's last path component (for readability in
/// `git for-each-ref`) plus a hash of remote and rev (for uniqueness).
pub fn pin_ref(remote: &str, rev: &str) -> String {
    let key = format!("{remote}\0{rev}");
    let hash = git2::Oid::hash_object(git2::ObjectType::Blob, key.as_bytes())
        .map(|oid| oid.to_string())
        .unwrap_or_default();
    format!(
        "{PIN_REF_PREFIX}{}-{}",
        repo_name(remote),
        short_hash(&hash)
    )
}

fn short_hash(hash: &str) -> &str {
    &hash[..hash.len().min(12)]
}

/// The last path component of a remote URL (or path), without `.git`,
/// sanitized into a valid single ref component.
fn repo_name(remote: &str) -> String {
    let last = remote
        .trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\', ':'])
        .next()
        .unwrap_or_default();
    let last = last.strip_suffix(".git").unwrap_or(last);
    let mut name: String = last
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '-'
            }
        })
        .collect();
    // Git ref-name rules: no "..", no leading '.'. (A trailing '.' or
    // ".lock" cannot occur: the hash suffix always follows.)
    while name.contains("..") {
        name = name.replace("..", "-.");
    }
    let name = name.trim_start_matches('.');
    if name.is_empty() {
        "repo".to_string()
    } else {
        name.to_string()
    }
}

pub fn short(sha: &str) -> &str {
    &sha[..sha.len().min(7)]
}

#[cfg(test)]
mod tests {
    use super::{PIN_REF_PREFIX, pin_ref, short};

    #[test]
    fn pin_refs_are_readable_and_valid() {
        for (remote, name) in [
            ("https://github.com/acme/widgets.git", "widgets"),
            ("git@gitlab.example.com:team/lib", "lib"),
            ("ssh://host/srv/repo.git/", "repo"),
            ("/abs/path/my repo", "my-repo"),
            ("C:\\repos\\win.git", "win"),
            ("../..hidden", "-.hidden"),
            ("https://host/", "host"),
            ("", "repo"),
        ] {
            let r = pin_ref(remote, "main");
            let rest = r.strip_prefix(PIN_REF_PREFIX).unwrap();
            assert_eq!(rest.rsplit_once('-').unwrap().0, name, "{remote}");
            assert!(git2::Reference::is_valid_name(&r), "{r}");
        }
    }

    #[test]
    fn pin_refs_are_keyed_by_remote_and_rev() {
        let url = "https://github.com/acme/widgets.git";
        assert_eq!(pin_ref(url, "main"), pin_ref(url, "main"));
        assert_ne!(pin_ref(url, "main"), pin_ref(url, "v1.0"));
        assert_ne!(
            pin_ref(url, "main"),
            pin_ref("https://github.com/fork/widgets.git", "main")
        );
        // The separator keeps remote/rev boundaries unambiguous.
        assert_ne!(pin_ref("a", "bc"), pin_ref("ab", "c"));
    }

    #[test]
    fn short_truncates_but_never_panics() {
        assert_eq!(short("0123456789abcdef"), "0123456");
        assert_eq!(short("abc"), "abc");
        assert_eq!(short(""), "");
    }
}
