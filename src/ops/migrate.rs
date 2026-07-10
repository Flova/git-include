//! `git include migrate [<path>...]`: convert git submodules into
//! included repositories, in place.
//!
//! Each submodule becomes an include pinned to the exact commit the
//! submodule recorded, so the migration never changes content — one host
//! commit per submodule. `.gitmodules` is updated (and removed once
//! empty), the submodule checkout with its embedded `.git` is replaced by
//! plain files, and leftover `submodule.*` config and `.git/modules`
//! clones are cleaned up.

use anyhow::{Context, Result, bail};

use crate::git::{Git, RevKind};
use crate::gitrepo::{GitRepoFile, MARKER_FILE, validate_subdir};
use crate::lfs;
use crate::ops::commit_message;
use crate::util::{pin_ref, short};

#[derive(Clone)]
struct Submodule {
    name: String,
    path: String,
    url: String,
}

pub fn run(git: &Git, paths: &[String], message: Option<&str>, no_lfs: bool) -> Result<()> {
    git.require_clean_worktree("migrate submodules")?;

    let all = parse_gitmodules(&gitmodules_content(git)?)?;
    if all.is_empty() {
        bail!("this repository has no submodules (no .gitmodules in HEAD)");
    }

    let targets: Vec<Submodule> = if paths.is_empty() {
        all
    } else {
        paths
            .iter()
            .map(|p| {
                all.iter().find(|s| s.path == *p).cloned().with_context(|| {
                    format!(
                        "'{p}' is not a submodule; known submodules:\n  {}",
                        all.iter()
                            .map(|s| s.path.as_str())
                            .collect::<Vec<_>>()
                            .join("\n  ")
                    )
                })
            })
            .collect::<Result<_>>()?
    };

    for sub in &targets {
        migrate_one(git, sub, message, no_lfs)?;
    }
    println!(
        "Migrated {} submodule(s). Track a branch instead of the pinned commit with:\n  \
         git include switch <dir> <branch>",
        targets.len()
    );
    Ok(())
}

fn migrate_one(git: &Git, sub: &Submodule, message: Option<&str>, no_lfs: bool) -> Result<()> {
    // Paths and URLs come from .gitmodules, which is repository content
    // someone else may have authored.
    validate_subdir(&sub.path)?;
    let head = git.head()?;
    let recorded = gitlink_at(git, &sub.path)?;

    eprintln!(
        "Migrating submodule '{}' (recorded commit {}) ...",
        sub.path,
        short(&recorded)
    );
    eprintln!("Fetching {} ...", sub.url);
    let (commit, kind) = git.fetch_rev(
        &sub.url,
        &recorded,
        Some(RevKind::Commit),
        &pin_ref(&sub.path),
    )?;

    let mut meta = GitRepoFile::new(&sub.url, &commit, &commit, Some(&head));
    meta.ref_kind_hint = Some(kind);
    let upstream_tree = git
        .rev_parse(&format!("{commit}^{{tree}}"))
        .context("fetched commit has no tree")?;
    let subtree = git.tree_with_blob(&upstream_tree, MARKER_FILE, meta.serialize().as_bytes())?;

    // New root tree: the gitlink replaced by the vendored subtree, and
    // the submodule's section dropped from .gitmodules (whole file gone
    // once no section remains).
    let mut root = git.root_tree_with_subtree(&sub.path, Some(&subtree))?;
    root = match remaining_gitmodules(git, &sub.name)? {
        Some(content) => git.tree_with_blob(&root, ".gitmodules", content.as_bytes())?,
        None => git.tree_without_entry(&root, ".gitmodules")?,
    };

    // Clear the submodule checkout (it contains its own .git) and the
    // metadata git keeps for it, then materialize the vendored files.
    let abs = git.toplevel.join(&sub.path);
    if abs.exists() {
        std::fs::remove_dir_all(&abs)
            .with_context(|| format!("cannot remove the submodule checkout at '{}'", sub.path))?;
    }
    let _ = std::fs::remove_dir_all(git.repo.path().join("modules").join(&sub.name));
    remove_submodule_config(git, &sub.name);

    // The index still holds the gitlink, whose commit object is not in
    // this repository — checkout cannot diff against it.
    git.index_remove_path(&sub.path)?;
    // Commit before materializing: libgit2 classifies paths as
    // submodules from .gitmodules in the worktree, index AND HEAD, and
    // refuses to check out plain files onto a submodule path. Only once
    // HEAD carries the updated .gitmodules is the path ordinary again.
    // (The index — the checkout baseline — is not moved by the commit.)
    git.commit_on_head(
        &commit_message(git, message, "migrate", &sub.path, &meta),
        &root,
    )?;
    git.apply_tree_prefix(&root, ".gitmodules")?;
    git.apply_tree_prefix(&root, &sub.path)?;
    // The path's old index entry was a gitlink; make the index match the
    // new HEAD exactly so nothing shows up as pending.
    git.reset_index_to_head()?;

    lfs::fetch_and_checkout(git, &sub.url, &commit, &sub.path, no_lfs);

    println!(
        "Migrated '{}' -> include of {} pinned to commit {}.",
        sub.path,
        sub.url,
        short(&commit)
    );
    Ok(())
}

/// The committed .gitmodules content ("" when the file does not exist).
fn gitmodules_content(git: &Git) -> Result<String> {
    let Some(oid) = git.rev_parse("HEAD:.gitmodules") else {
        return Ok(String::new());
    };
    let blob = git.repo.find_blob(git2::Oid::from_str(&oid)?)?;
    Ok(String::from_utf8_lossy(blob.content()).into_owned())
}

fn parse_gitmodules(content: &str) -> Result<Vec<Submodule>> {
    let mut out = Vec::new();
    let mut name: Option<String> = None;
    let mut path: Option<String> = None;
    let mut url: Option<String> = None;
    let flush = |name: &mut Option<String>,
                 path: &mut Option<String>,
                 url: &mut Option<String>,
                 out: &mut Vec<Submodule>| {
        if let (Some(n), Some(p), Some(u)) = (name.take(), path.take(), url.take()) {
            out.push(Submodule {
                name: n,
                path: p,
                url: u,
            });
        }
    };
    for line in content.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("[submodule \"") {
            flush(&mut name, &mut path, &mut url, &mut out);
            if let Some(n) = rest.strip_suffix("\"]") {
                name = Some(n.to_string());
            }
        } else if line.starts_with('[') {
            flush(&mut name, &mut path, &mut url, &mut out);
        } else if let Some((key, value)) = line.split_once('=') {
            match key.trim() {
                "path" => path = Some(value.trim().to_string()),
                "url" => url = Some(value.trim().to_string()),
                _ => {}
            }
        }
    }
    flush(&mut name, &mut path, &mut url, &mut out);
    Ok(out)
}

/// .gitmodules content with the section for `name` removed; None when no
/// submodule section remains (the file should be deleted).
fn remaining_gitmodules(git: &Git, name: &str) -> Result<Option<String>> {
    let content = gitmodules_content(git)?;
    let header = format!("[submodule \"{name}\"]");
    let mut kept = String::new();
    let mut skipping = false;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            skipping = trimmed == header;
        }
        if !skipping {
            kept.push_str(line);
            kept.push('\n');
        }
    }
    if kept.contains("[submodule ") {
        Ok(Some(kept))
    } else {
        Ok(None)
    }
}

/// The commit a submodule gitlink records in HEAD's tree.
fn gitlink_at(git: &Git, path: &str) -> Result<String> {
    let tree = git
        .repo
        .head()?
        .peel_to_tree()
        .context("HEAD has no tree")?;
    let entry = tree
        .get_path(std::path::Path::new(path))
        .with_context(|| format!("'{path}' is listed in .gitmodules but not present in HEAD"))?;
    if entry.filemode() != 0o160000 {
        bail!("'{path}' is not a submodule (no gitlink entry in HEAD)");
    }
    Ok(entry.id().to_string())
}

/// Best-effort removal of `submodule.<name>.*` from .git/config.
fn remove_submodule_config(git: &Git, name: &str) {
    if let Ok(mut cfg) = git.repo.config() {
        for key in ["url", "active", "branch", "update", "ignore"] {
            let _ = cfg.remove(&format!("submodule.{name}.{key}"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::parse_gitmodules;

    #[test]
    fn parses_typical_gitmodules() {
        let subs = parse_gitmodules(
            "[submodule \"one\"]\n\tpath = vendor/one\n\turl = https://x/one.git\n\
             [submodule \"two\"]\n    url = ../two.git\n    path = libs/two\n    branch = dev\n",
        )
        .unwrap();
        assert_eq!(subs.len(), 2);
        assert_eq!(subs[0].name, "one");
        assert_eq!(subs[0].path, "vendor/one");
        assert_eq!(subs[1].url, "../two.git");
        assert_eq!(subs[1].path, "libs/two");
    }

    #[test]
    fn incomplete_sections_are_ignored() {
        let subs =
            parse_gitmodules("[submodule \"broken\"]\n\tpath = only/path\n[other]\nkey = 1\n")
                .unwrap();
        assert!(subs.is_empty());
    }
}
