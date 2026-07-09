//! Git repository access built on the `git2` crate (libgit2 bindings).
//!
//! Everything git-include does — fetching, merging, tree surgery, commits,
//! pushing — goes through libgit2. The one exception lives in `lfs.rs`:
//! Git LFS is itself an external `git lfs` CLI extension with no library
//! API, so the (optional, best-effort) LFS integration shells out to it.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use git2::build::CheckoutBuilder;
use git2::{
    AutotagOption, Cred, CredentialType, Direction, FetchOptions, IndexEntry, IndexTime,
    MergeOptions, Oid, ProxyOptions, PushOptions, RemoteCallbacks, Repository, Sort, StatusOptions,
};

/// A handle to the host repository.
pub struct Git {
    pub repo: Repository,
    pub toplevel: PathBuf,
}

/// What kind of upstream revision an include tracks.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum RevKind {
    Branch,
    Tag,
    Commit,
}

impl RevKind {
    pub fn label(&self) -> &'static str {
        match self {
            RevKind::Branch => "branch",
            RevKind::Tag => "tag",
            RevKind::Commit => "commit",
        }
    }
}

/// Branch and tag heads advertised by a remote.
pub struct RemoteRefs {
    /// (sha, name) pairs.
    pub branches: Vec<(String, String)>,
    /// (sha, name) pairs; annotated tags are peeled to their commit.
    pub tags: Vec<(String, String)>,
}

/// Could this string be an abbreviated or full commit id?
pub fn looks_like_oid(s: &str) -> bool {
    (7..=40).contains(&s.len()) && s.chars().all(|c| c.is_ascii_hexdigit())
}

impl Git {
    /// Discover the repository containing `dir`.
    pub fn discover(dir: &Path) -> Result<Self> {
        let repo = Repository::discover(dir).context("not inside a git repository")?;
        let toplevel = repo
            .workdir()
            .context("this repository has no working tree (bare repository?)")?
            .to_path_buf();
        Ok(Git { repo, toplevel })
    }

    // ------------------------------------------------------------ basics --

    /// A string value from git config (repo, then global), if set.
    pub fn config_string(&self, key: &str) -> Option<String> {
        self.repo
            .config()
            .ok()
            .and_then(|c| c.get_string(key).ok())
            .filter(|s| !s.is_empty())
    }

    /// Resolve a revision string to a full object id; None if unresolvable.
    pub fn rev_parse(&self, spec: &str) -> Option<String> {
        self.repo
            .revparse_single(spec)
            .ok()
            .map(|o| o.id().to_string())
    }

    pub fn head(&self) -> Result<String> {
        let head = self
            .repo
            .head()
            .context("repository has no commits yet (unborn HEAD); make an initial commit first")?;
        Ok(head
            .peel_to_commit()
            .context("HEAD does not point at a commit")?
            .id()
            .to_string())
    }

    fn head_tree(&self) -> Result<git2::Tree<'_>> {
        self.repo.head()?.peel_to_tree().context("HEAD has no tree")
    }

    /// Object id of the tree at `path` in `rev`, or None if absent.
    pub fn tree_at(&self, rev: &str, path: &str) -> Option<String> {
        let obj = self.repo.revparse_single(&format!("{rev}:{path}")).ok()?;
        obj.peel_to_tree().ok().map(|t| t.id().to_string())
    }

    /// Does a commit object exist locally (e.g. fetched earlier)?
    pub fn has_commit(&self, oid: &str) -> bool {
        Oid::from_str(oid)
            .ok()
            .and_then(|o| self.repo.find_commit(o).ok())
            .is_some()
    }

    /// Is `descendant` a descendant of `ancestor`?
    pub fn is_descendant_of(&self, descendant: &str, ancestor: &str) -> bool {
        match (Oid::from_str(descendant), Oid::from_str(ancestor)) {
            (Ok(d), Ok(a)) => self.repo.graph_descendant_of(d, a).unwrap_or(false),
            _ => false,
        }
    }

    /// Number of commits reachable from `to` but not from `from`.
    pub fn count_range(&self, from: &str, to: &str) -> Result<usize> {
        Ok(self.walk_range(from, to)?.len())
    }

    /// All commits in `from..to`, oldest first.
    pub fn walk_range(&self, from: &str, to: &str) -> Result<Vec<String>> {
        let mut walk = self.repo.revwalk()?;
        walk.push(Oid::from_str(to)?)?;
        walk.hide(Oid::from_str(from)?)?;
        walk.set_sorting(Sort::TOPOLOGICAL | Sort::REVERSE)?;
        let mut out = Vec::new();
        for oid in walk {
            out.push(oid?.to_string());
        }
        Ok(out)
    }

    /// All commits reachable from `to`, oldest first.
    pub fn walk_all(&self, to: &str) -> Result<Vec<String>> {
        let mut walk = self.repo.revwalk()?;
        walk.push(Oid::from_str(to)?)?;
        walk.set_sorting(Sort::TOPOLOGICAL | Sort::REVERSE)?;
        let mut out = Vec::new();
        for oid in walk {
            out.push(oid?.to_string());
        }
        Ok(out)
    }

    // ------------------------------------------------------------ status --

    /// Require no staged or unstaged changes to tracked files (untracked
    /// files are fine, matching git-subrepo's rules).
    pub fn require_clean_worktree(&self, action: &str) -> Result<()> {
        let mut opts = StatusOptions::new();
        opts.include_untracked(false).include_ignored(false);
        let statuses = self.repo.statuses(Some(&mut opts))?;
        if !statuses.is_empty() {
            bail!(
                "cannot {action}: you have uncommitted changes.\n\
                 Commit or stash them first (`git status` for details)."
            );
        }
        Ok(())
    }

    /// Any uncommitted changes (including untracked files) under `subdir`?
    pub fn subdir_has_uncommitted(&self, subdir: &str) -> bool {
        let mut opts = StatusOptions::new();
        opts.include_untracked(true)
            .recurse_untracked_dirs(true)
            .include_ignored(false)
            .pathspec(subdir);
        self.repo
            .statuses(Some(&mut opts))
            .map(|s| !s.is_empty())
            .unwrap_or(false)
    }

    // ----------------------------------------------------------- remotes --

    /// Credential chain: ssh-agent, then git's credential helpers, then
    /// default (e.g. negotiate) — the standard setup used by cargo & co.
    fn callbacks(&self) -> RemoteCallbacks<'_> {
        let config = self.repo.config().ok();
        let mut cb = RemoteCallbacks::new();
        cb.credentials(move |url, username, allowed| {
            if allowed.contains(CredentialType::USERNAME) {
                return Cred::username(username.unwrap_or("git"));
            }
            if allowed.contains(CredentialType::SSH_KEY) {
                return Cred::ssh_key_from_agent(username.unwrap_or("git"));
            }
            if allowed.contains(CredentialType::USER_PASS_PLAINTEXT)
                && let Some(cfg) = &config
            {
                return Cred::credential_helper(cfg, url, username);
            }
            Cred::default()
        });
        cb
    }

    fn open_remote(&self, name_or_url: &str) -> Result<git2::Remote<'_>> {
        match self.repo.find_remote(name_or_url) {
            Ok(r) => Ok(r),
            Err(_) => self
                .repo
                .remote_anonymous(name_or_url)
                .with_context(|| format!("invalid remote '{name_or_url}'")),
        }
    }

    /// Ask the remote which branch its HEAD points at.
    pub fn remote_default_branch(&self, remote: &str) -> Result<String> {
        let mut r = self.open_remote(remote)?;
        r.connect_auth(Direction::Fetch, Some(self.callbacks()), None)
            .with_context(|| format!("could not connect to {remote}"))?;
        let buf = r.default_branch().with_context(|| {
            format!("could not determine the default branch of {remote}; pass --branch explicitly")
        })?;
        let name = std::str::from_utf8(&buf)?
            .strip_prefix("refs/heads/")
            .context("remote HEAD is not a branch")?
            .to_string();
        Ok(name)
    }

    /// List the branch and tag heads a remote advertises.
    pub fn remote_refs(&self, remote: &str) -> Result<RemoteRefs> {
        let mut r = self.open_remote(remote)?;
        r.connect_auth(Direction::Fetch, Some(self.callbacks()), None)
            .with_context(|| format!("could not connect to {remote}"))?;
        let mut branches = Vec::new();
        let mut tags: Vec<(String, String)> = Vec::new();
        let mut peeled: Vec<(String, String)> = Vec::new();
        for head in r.list()? {
            let name = head.name();
            if let Some(b) = name.strip_prefix("refs/heads/") {
                branches.push((head.oid().to_string(), b.to_string()));
            } else if let Some(t) = name.strip_prefix("refs/tags/") {
                match t.strip_suffix("^{}") {
                    // The peeled entry gives the commit an annotated tag
                    // points at — prefer it over the tag object id.
                    Some(t) => peeled.push((head.oid().to_string(), t.to_string())),
                    None => tags.push((head.oid().to_string(), t.to_string())),
                }
            }
        }
        for (sha, name) in peeled {
            if let Some(entry) = tags.iter_mut().find(|(_, n)| *n == name) {
                entry.0 = sha;
            } else {
                tags.push((sha, name));
            }
        }
        Ok(RemoteRefs { branches, tags })
    }

    fn fetch_options(&self) -> FetchOptions<'_> {
        let mut proxy = ProxyOptions::new();
        proxy.auto();
        let mut fo = FetchOptions::new();
        fo.remote_callbacks(self.callbacks())
            .proxy_options(proxy)
            .download_tags(AutotagOption::None);
        fo
    }

    fn fetch_refspecs(&self, remote: &str, refspecs: &[String]) -> Result<()> {
        let mut r = self.open_remote(remote)?;
        let specs: Vec<&str> = refspecs.iter().map(String::as_str).collect();
        r.fetch(&specs, Some(&mut self.fetch_options()), None)
            .with_context(|| format!("could not fetch from {remote}"))?;
        Ok(())
    }

    /// Fetch `rev` — a branch, tag, or commit id — from `remote`, returning
    /// the resolved commit and what kind of revision it turned out to be.
    /// `expect` restricts the lookup (e.g. `--tag` on the command line).
    /// The commit is pinned under `pin_ref` so it survives `git gc` and is
    /// reusable for offline status/diff.
    pub fn fetch_rev(
        &self,
        remote: &str,
        rev: &str,
        expect: Option<RevKind>,
        pin_ref: &str,
    ) -> Result<(String, RevKind)> {
        let refs = self.remote_refs(remote)?;
        let want = |k: RevKind| expect.is_none() || expect == Some(k);

        if want(RevKind::Branch) && refs.branches.iter().any(|(_, n)| n == rev) {
            self.delete_ref(pin_ref);
            self.fetch_refspecs(remote, &[format!("+refs/heads/{rev}:{pin_ref}")])?;
            let sha = self
                .repo
                .refname_to_id(pin_ref)
                .with_context(|| format!("fetched branch '{rev}' does not resolve"))?
                .to_string();
            return Ok((sha, RevKind::Branch));
        }

        if want(RevKind::Tag) && refs.tags.iter().any(|(_, n)| n == rev) {
            self.delete_ref(pin_ref);
            self.fetch_refspecs(remote, &[format!("+refs/tags/{rev}:{pin_ref}")])?;
            // Annotated tags need peeling to the commit they point at.
            let sha = self
                .rev_parse(&format!("{pin_ref}^{{commit}}"))
                .with_context(|| format!("tag '{rev}' does not point at a commit"))?;
            self.set_ref(pin_ref, &sha)?;
            return Ok((sha, RevKind::Tag));
        }

        if want(RevKind::Commit) && looks_like_oid(rev) {
            // Try a direct fetch-by-id first (works on servers that allow
            // it); otherwise fetch all heads and tags and resolve locally.
            let direct = rev.len() == 40
                && self
                    .fetch_refspecs(remote, &[format!("+{rev}:{pin_ref}")])
                    .is_ok();
            if !direct && self.rev_parse(&format!("{rev}^{{commit}}")).is_none() {
                self.fetch_refspecs(
                    remote,
                    &[
                        "+refs/heads/*:refs/include/scan/heads/*".to_string(),
                        "+refs/tags/*:refs/include/scan/tags/*".to_string(),
                    ],
                )?;
                self.delete_scan_refs();
            }
            if let Some(sha) = self.rev_parse(&format!("{rev}^{{commit}}")) {
                self.set_ref(pin_ref, &sha)?;
                return Ok((sha, RevKind::Commit));
            }
        }

        match expect {
            Some(kind) => bail!("'{rev}' is not a {} on {remote}", kind.label()),
            None => bail!("'{rev}' is not a branch, tag, or commit on {remote}"),
        }
    }

    /// Remove the temporary refs used when scanning a remote for a commit.
    fn delete_scan_refs(&self) {
        let names: Vec<String> = self
            .repo
            .references()
            .map(|refs| {
                refs.filter_map(|r| r.ok())
                    .filter_map(|r| r.name().ok().map(str::to_string))
                    .filter(|n| n.starts_with("refs/include/scan/"))
                    .collect()
            })
            .unwrap_or_default();
        for name in names {
            self.delete_ref(&name);
        }
    }

    /// Best-effort fetch of a single commit by id (used to recover base
    /// commits that are not reachable from any branch head). Failure is
    /// fine; callers re-check object presence afterwards.
    pub fn try_fetch_commit(&self, remote: &str, oid: &str) {
        if let Ok(mut r) = self.open_remote(remote) {
            let _ = r.fetch(&[oid], Some(&mut self.fetch_options()), None);
        }
    }

    /// Push `commit` to `refs/heads/<branch>` on the remote.
    pub fn push_commit(&self, remote: &str, commit: &str, branch: &str) -> Result<()> {
        // libgit2 push refspecs need a local ref as source; use a
        // throwaway one.
        let tmp_name = format!("refs/include/push-tmp-{}", std::process::id());
        let tmp =
            self.repo
                .reference(&tmp_name, Oid::from_str(commit)?, true, "git-include push")?;
        let result = (|| -> Result<()> {
            let mut rejection: Option<String> = None;
            {
                let mut cb = self.callbacks();
                cb.push_update_reference(|refname, status| {
                    if let Some(msg) = status {
                        rejection = Some(format!("{refname}: {msg}"));
                    }
                    Ok(())
                });
                let mut proxy = ProxyOptions::new();
                proxy.auto();
                let mut po = PushOptions::new();
                po.remote_callbacks(cb).proxy_options(proxy);
                let mut r = self.open_remote(remote)?;
                let refspec = format!("{tmp_name}:refs/heads/{branch}");
                r.push(&[&refspec], Some(&mut po))
                    .with_context(|| format!("failed to push to {remote} ({branch})"))?;
            }
            if let Some(rej) = rejection {
                bail!("push to {remote} was rejected: {rej}");
            }
            Ok(())
        })();
        let mut tmp = tmp;
        let _ = tmp.delete();
        result
    }

    /// Create or move a ref to `oid`.
    pub fn set_ref(&self, name: &str, oid: &str) -> Result<()> {
        self.repo
            .reference(name, Oid::from_str(oid)?, true, "git-include")?;
        Ok(())
    }

    pub fn delete_ref(&self, name: &str) {
        if let Ok(mut r) = self.repo.find_reference(name) {
            let _ = r.delete();
        }
    }

    // ------------------------------------------------------ tree surgery --

    /// Remove the entry `name` from `tree`, returning the new tree id.
    /// (Used to strip `.gitrepo` before anything travels upstream.)
    pub fn tree_without_entry(&self, tree: &str, name: &str) -> Result<String> {
        let tree = self.repo.find_tree(Oid::from_str(tree)?)?;
        let mut builder = self.repo.treebuilder(Some(&tree))?;
        if builder.get(name)?.is_some() {
            builder.remove(name)?;
        }
        Ok(builder.write()?.to_string())
    }

    /// Insert/replace a blob entry `name` with `content` in `tree`.
    pub fn tree_with_blob(&self, tree: &str, name: &str, content: &[u8]) -> Result<String> {
        let tree = self.repo.find_tree(Oid::from_str(tree)?)?;
        let blob = self.repo.blob(content)?;
        let mut builder = self.repo.treebuilder(Some(&tree))?;
        builder.insert(name, blob, 0o100644)?;
        Ok(builder.write()?.to_string())
    }

    /// Return HEAD's root tree with the subtree at `prefix` replaced by
    /// `subtree` (or removed entirely when `subtree` is None).
    pub fn root_tree_with_subtree(&self, prefix: &str, subtree: Option<&str>) -> Result<String> {
        let root = self.head_tree()?;
        let parts: Vec<&str> = prefix.split('/').collect();
        let sub = subtree.map(Oid::from_str).transpose()?;
        let oid = self.graft(Some(&root), &parts, sub)?;
        Ok(oid.to_string())
    }

    fn graft(
        &self,
        tree: Option<&git2::Tree<'_>>,
        parts: &[&str],
        sub: Option<Oid>,
    ) -> Result<Oid> {
        let mut builder = self.repo.treebuilder(tree)?;
        let name = parts[0];
        if parts.len() == 1 {
            match sub {
                Some(oid) => {
                    builder.insert(name, oid, 0o040000)?;
                }
                None => {
                    if builder.get(name)?.is_some() {
                        builder.remove(name)?;
                    }
                }
            }
        } else {
            let existing = match builder.get(name)? {
                Some(entry) => Some(self.repo.find_tree(entry.id())?),
                None => None,
            };
            let child = self.graft(existing.as_ref(), &parts[1..], sub)?;
            builder.insert(name, child, 0o040000)?;
        }
        Ok(builder.write()?)
    }

    // -------------------------------------------------- worktree & index --

    /// Make the working tree and index match `root_tree` for all paths
    /// under `prefix` (files outside the prefix are never touched). The
    /// current index is the baseline, so this must run *before* HEAD moves.
    pub fn apply_tree_prefix(&self, root_tree: &str, prefix: &str) -> Result<()> {
        let obj = self.repo.find_object(Oid::from_str(root_tree)?, None)?;
        let mut co = CheckoutBuilder::new();
        co.force().path(prefix);
        self.repo
            .checkout_tree(&obj, Some(&mut co))
            .with_context(|| format!("failed to materialize new content in '{prefix}'"))?;
        prune_empty_dirs(&self.toplevel.join(prefix));
        Ok(())
    }

    // ------------------------------------------------------------ commit --

    /// Commit `root_tree` on the current branch with the configured user
    /// as author and committer.
    pub fn commit_on_head(&self, message: &str, root_tree: &str) -> Result<String> {
        let sig = self
            .repo
            .signature()
            .context("cannot determine committer; set user.name and user.email in git config")?;
        let parent = self.repo.head()?.peel_to_commit()?;
        let tree = self.repo.find_tree(Oid::from_str(root_tree)?)?;
        let oid = self
            .repo
            .commit(Some("HEAD"), &sig, &sig, message, &tree, &[&parent])?;
        Ok(oid.to_string())
    }

    /// Create a commit object with `tree`, an optional parent (None makes a
    /// root commit), and the author + message of `original` (committer is
    /// the configured user). No ref moves.
    pub fn replay_commit(
        &self,
        original: &str,
        tree: &str,
        parent: Option<&str>,
    ) -> Result<String> {
        let orig = self.repo.find_commit(Oid::from_str(original)?)?;
        let author = orig.author().to_owned();
        let committer = self
            .repo
            .signature()
            .context("cannot determine committer; set user.name and user.email in git config")?;
        let tree = self.repo.find_tree(Oid::from_str(tree)?)?;
        let parent = parent
            .map(|p| self.repo.find_commit(Oid::from_str(p)?))
            .transpose()?;
        let parents: Vec<&git2::Commit<'_>> = parent.iter().collect();
        let message = String::from_utf8_lossy(orig.message_bytes()).into_owned();
        let oid = self
            .repo
            .commit(None, &author, &committer, &message, &tree, &parents)?;
        Ok(oid.to_string())
    }

    /// Create a commit object with `tree` and the configured user as both
    /// author and committer. No ref moves.
    pub fn new_commit(&self, tree: &str, parent: &str, message: &str) -> Result<String> {
        let sig = self
            .repo
            .signature()
            .context("cannot determine committer; set user.name and user.email in git config")?;
        let tree = self.repo.find_tree(Oid::from_str(tree)?)?;
        let parent = self.repo.find_commit(Oid::from_str(parent)?)?;
        let oid = self
            .repo
            .commit(None, &sig, &sig, message, &tree, &[&parent])?;
        Ok(oid.to_string())
    }

    /// The id of the empty tree.
    pub fn empty_tree(&self) -> Result<String> {
        Ok(self.repo.treebuilder(None)?.write()?.to_string())
    }

    pub fn commit_summary(&self, oid: &str) -> String {
        Oid::from_str(oid)
            .ok()
            .and_then(|o| self.repo.find_commit(o).ok())
            .and_then(|c| {
                c.summary_bytes()
                    .map(|b| String::from_utf8_lossy(b).into_owned())
            })
            .unwrap_or_default()
    }

    // ------------------------------------------------------------- merge --

    /// Three-way merge of subdirectory trees. Returns the merged tree id
    /// and the list of conflicted paths; conflicting files carry standard
    /// conflict markers in the merged tree.
    pub fn merge_trees_3way(
        &self,
        base: &str,
        ours: &str,
        theirs: &str,
    ) -> Result<(String, Vec<String>)> {
        let base = self.repo.find_tree(Oid::from_str(base)?)?;
        let ours = self.repo.find_tree(Oid::from_str(ours)?)?;
        let theirs = self.repo.find_tree(Oid::from_str(theirs)?)?;
        let mut index = self
            .repo
            .merge_trees(&base, &ours, &theirs, Some(&MergeOptions::new()))?;
        if !index.has_conflicts() {
            return Ok((index.write_tree_to(&self.repo)?.to_string(), Vec::new()));
        }

        let conflicts: Vec<git2::IndexConflict> =
            index.conflicts()?.collect::<Result<_, git2::Error>>()?;
        let mut names = Vec::new();
        for conflict in &conflicts {
            let side = conflict
                .our
                .as_ref()
                .or(conflict.their.as_ref())
                .or(conflict.ancestor.as_ref())
                .context("conflict with no entries")?;
            let path = String::from_utf8_lossy(&side.path).into_owned();

            let content = self.conflict_content(conflict)?;
            let blob = self.repo.blob(&content)?;
            for stage in 1..=3 {
                let _ = index.remove(Path::new(&path), stage);
            }
            index.add(&IndexEntry {
                ctime: IndexTime::new(0, 0),
                mtime: IndexTime::new(0, 0),
                dev: 0,
                ino: 0,
                mode: side.mode,
                uid: 0,
                gid: 0,
                file_size: content.len() as u32,
                id: blob,
                flags: 0,
                flags_extended: 0,
                path: path.clone().into_bytes(),
            })?;
            names.push(path);
        }
        names.sort();
        Ok((index.write_tree_to(&self.repo)?.to_string(), names))
    }

    /// Content (with conflict markers) for a conflicted index entry.
    fn conflict_content(&self, conflict: &git2::IndexConflict) -> Result<Vec<u8>> {
        if let (Some(ours), Some(theirs)) = (&conflict.our, &conflict.their) {
            // For add/add conflicts there is no ancestor; merge against an
            // empty file so both sides end up between conflict markers.
            let synthetic;
            let ancestor = match &conflict.ancestor {
                Some(a) => a,
                None => {
                    synthetic = IndexEntry {
                        ctime: IndexTime::new(0, 0),
                        mtime: IndexTime::new(0, 0),
                        dev: 0,
                        ino: 0,
                        mode: ours.mode,
                        uid: 0,
                        gid: 0,
                        file_size: 0,
                        id: self.repo.blob(b"")?,
                        flags: 0,
                        flags_extended: 0,
                        path: ours.path.clone(),
                    };
                    &synthetic
                }
            };
            if let Ok(merged) = self
                .repo
                .merge_file_from_index(ancestor, ours, theirs, None)
            {
                return Ok(merged.content().to_vec());
            }
        }
        // Delete/modify or binary conflict: keep our side if it exists,
        // otherwise theirs (the path stays listed as conflicted either way).
        let side = conflict
            .our
            .as_ref()
            .or(conflict.their.as_ref())
            .context("conflict with no entries")?;
        Ok(self.repo.find_blob(side.id)?.content().to_vec())
    }
}

/// Remove now-empty directories left behind after files were deleted.
fn prune_empty_dirs(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            prune_empty_dirs(&entry.path());
        }
    }
    // remove_dir only succeeds on empty directories.
    let _ = std::fs::remove_dir(dir);
}
