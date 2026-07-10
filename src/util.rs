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

/// Ref under which fetched upstream commits are pinned, so they survive
/// `git gc` and enable offline `status`/`diff`.
///
/// The subdirectory is flattened into a single ref component ('/' becomes
/// "--"): a ref hierarchy would make the pin of an include collide with
/// the pin of an include nested inside it — a loose ref cannot be both a
/// file and a parent directory.
pub fn pin_ref(subdir: &str) -> String {
    let sanitized: String = subdir
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '/' || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let mut name = sanitized
        .trim_matches('/')
        .split('/')
        .filter(|c| !c.is_empty())
        .collect::<Vec<_>>()
        .join("--");
    // Git ref-name rules: no "..", no leading '.', no trailing '.' or
    // '.lock' (libgit2 rejects such refs outright).
    while name.contains("..") {
        name = name.replace("..", "-.");
    }
    if name.starts_with('.') {
        name.insert(0, '-');
    }
    if name.ends_with('.') || name.ends_with(".lock") {
        name.push('-');
    }
    format!("refs/include/{name}")
}

pub fn short(sha: &str) -> &str {
    &sha[..sha.len().min(7)]
}

#[cfg(test)]
mod tests {
    use super::{pin_ref, short};

    #[test]
    fn pin_refs_are_valid_and_stable() {
        assert_eq!(pin_ref("vendor/lib"), "refs/include/vendor--lib");
        assert_eq!(pin_ref("with space"), "refs/include/with-space");
        assert_eq!(pin_ref("weird~^:name"), "refs/include/weird---name");
        assert_eq!(
            pin_ref("/leading/trailing/"),
            "refs/include/leading--trailing"
        );
        assert_eq!(pin_ref(".hidden"), "refs/include/-.hidden");
        assert_eq!(pin_ref("foo.lock"), "refs/include/foo.lock-");
        assert_eq!(pin_ref("a..b"), "refs/include/a-.b");
    }

    #[test]
    fn nested_include_pins_do_not_collide_with_their_parent() {
        // "refs/include/a" as a loose ref would block "refs/include/a/b"
        // from ever being created; the flattened names have no hierarchy.
        let outer = pin_ref("libs/b");
        let nested = pin_ref("libs/b/vendor/c");
        assert!(!nested.starts_with(&format!("{outer}/")));
    }

    #[test]
    fn short_truncates_but_never_panics() {
        assert_eq!(short("0123456789abcdef"), "0123456");
        assert_eq!(short("abc"), "abc");
        assert_eq!(short(""), "");
    }
}
