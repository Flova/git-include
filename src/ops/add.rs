use anyhow::{Context, Result, bail};

use crate::git::{Git, RevKind};
use crate::gitrepo::{GitRepoFile, MARKER_FILE, validate_subdir};
use crate::lfs;
use crate::ops::commit_message;
use crate::util::{pin_ref, short};

pub struct AddOptions<'a> {
    /// Branch, tag, or commit to track, with an optional kind restriction
    /// from the `--branch`/`--tag`/`--commit` flags.
    pub rev: Option<(&'a str, RevKind)>,
    pub message: Option<&'a str>,
    pub no_lfs: bool,
}

pub fn run(git: &Git, remote: &str, subdir: &str, opts: &AddOptions<'_>) -> Result<()> {
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

    let (rev, expect) = match opts.rev {
        Some((rev, kind)) => (rev.to_string(), Some(kind)),
        None => {
            let b = git.remote_default_branch(remote)?;
            eprintln!("No revision given; using upstream default branch '{b}'.");
            (b, Some(RevKind::Branch))
        }
    };

    eprintln!("Fetching {remote} ({rev}) ...");
    let (commit, kind) = git.fetch_rev(remote, &rev, expect, &pin_ref(subdir))?;

    let meta = GitRepoFile::new(remote, &rev, &commit, Some(&head));
    let upstream_tree = git
        .rev_parse(&format!("{commit}^{{tree}}"))
        .context("fetched commit has no tree")?;
    let subtree = git.tree_with_blob(&upstream_tree, MARKER_FILE, meta.serialize().as_bytes())?;

    let root = git.root_tree_with_subtree(subdir, Some(&subtree))?;
    git.apply_tree_prefix(&root, subdir)?;
    git.commit_on_head(
        &commit_message(git, opts.message, "add", subdir, &meta),
        &root,
    )?;

    lfs::fetch_and_checkout(git, remote, &commit, subdir, opts.no_lfs);

    match kind {
        RevKind::Branch => println!(
            "Added '{subdir}' from {remote} (branch {rev}, commit {}).",
            short(&commit)
        ),
        _ => println!(
            "Added '{subdir}' from {remote}, pinned to {} '{rev}' (commit {}).",
            kind.label(),
            short(&commit)
        ),
    }
    Ok(())
}
