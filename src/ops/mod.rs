pub mod add;
pub mod branches;
pub mod diff;
pub mod init;
pub mod list;
pub mod pull;
pub mod push;
pub mod remove;
pub mod selfupdate;
pub mod status;

use anyhow::{Context, Result, bail};

use crate::git::Git;
use crate::gitrepo::{GitRepoFile, MARKER_FILE};
use crate::util::pin_ref;

/// An existing include: a subdirectory with a `.gitrepo` marker file.
pub struct Include<'a> {
    pub git: &'a Git,
    /// Repository-relative path of the included directory.
    pub subdir: String,
    pub meta: GitRepoFile,
}

impl<'a> Include<'a> {
    pub fn load(git: &'a Git, subdir: &str) -> Result<Self> {
        let marker = git.toplevel.join(subdir).join(MARKER_FILE);
        if !marker.exists() {
            bail!(
                "'{subdir}' is not an included repository (no {MARKER_FILE} file found).\n\
                 Use `git include list` to see all included repositories."
            );
        }
        let meta = GitRepoFile::load(&marker)?;
        Ok(Include {
            git,
            subdir: subdir.to_string(),
            meta,
        })
    }

    pub fn pin_ref(&self) -> String {
        pin_ref(&self.subdir)
    }

    /// Tree of the included directory in HEAD, with the `.gitrepo` marker
    /// stripped (i.e. the tree as upstream would see it).
    pub fn local_tree_stripped(&self) -> Result<String> {
        let tree = self
            .git
            .tree_at("HEAD", &self.subdir)
            .with_context(|| format!("'{}' does not exist in HEAD", self.subdir))?;
        self.git.tree_without_entry(&tree, MARKER_FILE)
    }

    /// Tree of the upstream commit recorded in the marker file, if the
    /// commit object is available locally.
    pub fn base_tree(&self) -> Option<String> {
        self.git
            .rev_parse(&format!("{}^{{tree}}", self.meta.commit))
    }

    /// Make sure the upstream base commit recorded in the marker exists
    /// locally, fetching it from the remote if necessary.
    pub fn ensure_base_commit(&self) -> Result<()> {
        if self.git.has_commit(&self.meta.commit) {
            return Ok(());
        }
        self.git
            .try_fetch_commit(&self.meta.remote, &self.meta.commit);
        if !self.git.has_commit(&self.meta.commit) {
            bail!(
                "the upstream commit {} recorded in {}/{MARKER_FILE} is not available,\n\
                 not even after fetching from {}. This usually means upstream history\n\
                 was rewritten (force-push). Re-add the include to recover:\n\
                 git include remove {} && git include add {} {}",
                self.meta.commit,
                self.subdir,
                self.meta.remote,
                self.subdir,
                self.meta.remote,
                self.subdir,
            );
        }
        Ok(())
    }

    /// Latest known upstream commit: the pin ref maintained by fetches.
    pub fn pinned_upstream(&self) -> Option<String> {
        self.git.rev_parse(&self.pin_ref())
    }

    /// Commit `subtree` (already carrying the right marker file) as the new
    /// content of the included directory: updates worktree + index, then
    /// creates one commit on HEAD. A no-op (returning the current HEAD)
    /// when nothing would change.
    pub fn commit_subtree(&self, subtree: &str, message: &str) -> Result<String> {
        let root = self
            .git
            .root_tree_with_subtree(&self.subdir, Some(subtree))?;
        self.git.apply_tree_prefix(&root, &self.subdir)?;
        if self.git.rev_parse("HEAD^{tree}").as_deref() == Some(root.as_str()) {
            return self.git.head();
        }
        self.git.commit_on_head(message, &root)
    }
}

/// Find every `.gitrepo` marker tracked in the index (includes nested
/// ones), returning the repo-relative directories, sorted.
pub fn find_all_includes(git: &Git) -> Result<Vec<String>> {
    let index = git.repo.index()?;
    let suffix = format!("/{MARKER_FILE}").into_bytes();
    let mut dirs: Vec<String> = index
        .iter()
        .filter_map(|entry| {
            let path = entry.path;
            path.strip_suffix(suffix.as_slice())
                .map(|dir| String::from_utf8_lossy(dir).into_owned())
        })
        .collect();
    dirs.sort();
    Ok(dirs)
}

/// The default template for sync commit messages (see [`commit_message`]).
pub const DEFAULT_COMMIT_TEMPLATE: &str = "git include {{ action }} {{ subdir }}\n\
     \n\
     include:\n\
     \x20 subdir: \"{{ subdir }}\"\n\
     \x20 remote: \"{{ remote }}\"\n\
     \x20 ref: \"{{ ref }}\"\n\
     \x20 commit: \"{{ commit }}\"\n\
     git-include-version: {{ version }}";

/// Render the message for a sync commit. Template precedence: `--message`
/// on the command line, then the `include.commitTemplate` git config key,
/// then [`DEFAULT_COMMIT_TEMPLATE`]. Templates use `{{ variable }}`
/// substitution (see the `template` module).
pub fn commit_message(
    git: &Git,
    cli_template: Option<&str>,
    action: &str,
    inc_subdir: &str,
    meta: &GitRepoFile,
) -> String {
    let template = cli_template
        .map(str::to_string)
        .or_else(|| git.config_string("include.commitTemplate"))
        .unwrap_or_else(|| DEFAULT_COMMIT_TEMPLATE.to_string());
    crate::template::render(
        &template,
        &[
            ("action", action.to_string()),
            ("subdir", inc_subdir.to_string()),
            ("remote", meta.remote.clone()),
            ("ref", meta.branch.clone()),
            ("branch", meta.branch.clone()),
            ("commit", meta.commit.clone()),
            ("short_commit", crate::util::short(&meta.commit).to_string()),
            ("version", env!("CARGO_PKG_VERSION").to_string()),
        ],
    )
}
