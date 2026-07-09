use anyhow::{Context, Result, bail};

use crate::git::Git;
use crate::gitrepo::{GitRepoFile, MARKER_FILE, validate_subdir};
use crate::lfs;
use crate::ops::commit_message;
use crate::util::{pin_ref, short};

pub fn run(
    git: &Git,
    remote: &str,
    subdir: &str,
    branch: Option<&str>,
    no_lfs: bool,
) -> Result<()> {
    validate_subdir(subdir)?;
    git.require_clean_worktree("add an included repository")?;
    let head = git.head()?;

    let abs = git.toplevel.join(subdir);
    if abs.join(MARKER_FILE).exists() {
        bail!("'{subdir}' is already an included repository (it has a {MARKER_FILE} file)");
    }
    if abs.exists()
        && abs
            .read_dir()
            .map(|mut d| d.next().is_some())
            .unwrap_or(true)
    {
        bail!("'{subdir}' already exists and is not empty");
    }
    if git.tree_at("HEAD", subdir).is_some() {
        bail!("'{subdir}' already contains tracked files");
    }

    let branch = match branch {
        Some(b) => b.to_string(),
        None => {
            let b = git.remote_default_branch(remote)?;
            eprintln!("No branch given; using upstream default branch '{b}'.");
            b
        }
    };

    eprintln!("Fetching {remote} ({branch}) ...");
    let commit = git.fetch_branch(remote, &branch, &pin_ref(subdir))?;

    let meta = GitRepoFile::new(remote, &branch, &commit, Some(&head));
    let upstream_tree = git
        .rev_parse(&format!("{commit}^{{tree}}"))
        .context("fetched commit has no tree")?;
    let subtree = git.tree_with_blob(&upstream_tree, MARKER_FILE, meta.serialize().as_bytes())?;

    let root = git.root_tree_with_subtree(subdir, Some(&subtree))?;
    git.apply_tree_prefix(&root, subdir)?;
    git.commit_on_head(&commit_message("add", subdir, &meta), &root)?;

    lfs::fetch_and_checkout(git, remote, &commit, subdir, no_lfs);

    println!(
        "Added '{subdir}' from {remote} (branch {branch}, commit {}).",
        short(&commit)
    );
    Ok(())
}
