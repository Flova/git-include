# git-include

**Vendor external git repositories as plain files — with full two-way sync.**

`git-include` is a modern, single-binary alternative to
[git-subrepo](https://github.com/ingydotnet/git-subrepo), written in Rust. It
inlines an upstream repository into a subdirectory of your repository, plus one
small marker file. That's the whole model:

- **Collaborators need nothing.** They `git clone` and get working code. No
  `--recursive`, no `submodule update`, no git-include installation required.
  Only the person syncing with upstream needs the tool.
- **Two-way sync.** `git include pull` merges new upstream work into your tree;
  `git include push` rebuilds upstream history from your commits — each host
  commit that touched the directory becomes an individual upstream commit
  with its original message and author (even commits made before a pull),
  and the marker file never leaks upstream.
- **git-subrepo compatible.** The marker file is the same `.gitrepo` format.
  You can adopt a repository that already uses git-subrepo, or hand one back.
- **Export built in.** `git include init` turns any ordinary directory into
  a new included repository, extracting its full history from your commits —
  ready to push to its own (even empty) repository.
- **First-class Git LFS support**, painless **branch switching**, quick
  **status/diff against upstream**, **nested includes**, and **tab completion**
  out of the box.

```console
$ git include add https://github.com/example/widgets vendor/widgets
$ git include status
$ git include pull vendor/widgets      # get new upstream work
$ git include push vendor/widgets      # contribute your changes back
```

---

## Table of contents

- [Why not submodules / subtree / subrepo?](#why-not-submodules--subtree--subrepo)
- [Installation](#installation)
- [Tab completion](#tab-completion)
- [Quickstart](#quickstart)
- [Command reference](#command-reference)
- [Pinning to tags and commits](#pinning-to-tags-and-commits)
- [Custom commit messages](#custom-commit-messages)
- [The `.gitrepo` marker file](#the-gitrepo-marker-file)
- [Git LFS](#git-lfs)
- [Exporting a directory into its own repository](#exporting-a-directory-into-its-own-repository)
- [Nested includes](#nested-includes)
- [Handling merge conflicts](#handling-merge-conflicts)
- [How it works](#how-it-works)
- [FAQ](#faq)

---

## Why not submodules / subtree / subrepo?

|                                      | submodule | subtree | subrepo | **git-include** |
| ------------------------------------ | :-------: | :-----: | :-----: | :-------------: |
| Plain `git clone` just works         |     ✗     |    ✓    |    ✓    |        ✓        |
| Collaborators need no extra tool     |     ✗     |    ✓    |    ✓    |        ✓        |
| Clean host history (no merge noise)  |     ✓     |    ✗    |    ✓    |        ✓        |
| Two-way sync (pull *and* push)       |     ✓     |   (✓)   |    ✓    |        ✓        |
| Individual commits pushed upstream   |     ✓     |    ✓    |    ✓    |        ✓        |
| No hidden state outside the worktree |     ✗     |    ✗    |    ✓    |        ✓        |
| Single static binary                 |    n/a    |   n/a   | ✗ (bash)|        ✓        |
| Git LFS aware                        |     ✓     |    ✗    |    ✗    |        ✓        |
| One-command branch switching         |     ✗     |    ✗    |    ✗    |        ✓        |
| Status/diff against upstream         |     ✗     |    ✗    |   (✓)   |        ✓        |
| Nested vendored repos                |     ✓     |    ✗    |   (✓)   |        ✓        |

The fundamental idea is the same as git-subrepo: **the vendored code is just
files in your repository**, and a marker file records where they came from and
which upstream commit they correspond to. Everything else — merging, pushing,
diffing — is derived from that.

Compared to git-subrepo, git-include is a compiled binary (built on libgit2
via the `git2` crate) instead of ~2000 lines of bash, and never creates
temporary branches, worktrees, or clones in your repository: your branches
and your working tree stay untouched except for the one subdirectory being
operated on.

## Installation

**Linux / macOS — one-liner:**

```console
$ curl -fsSL https://raw.githubusercontent.com/flova/git-include/main/install.sh | bash
```

The script detects your platform, downloads the latest release binary and
installs it to `~/.local/bin` (or `/usr/local/bin` as root). Pin a version
with `GIT_INCLUDE_VERSION=v0.1.0`, change the directory with
`GIT_INCLUDE_BIN_DIR`. Update any time — the binary updates itself:

```console
$ git include self-update            # or --version vX.Y.Z, or -n to preview
```

**Windows:** download the MSI installer from the
[latest release](https://github.com/flova/git-include/releases/latest) —
it installs `git-include.exe` and puts it on `PATH`. (`self-update` works
on Windows too.)

**Conda:** every release ships a `.conda` package (see the release assets;
installable into a channel of your choice — the recipe lives in
`conda/recipe.yaml`).

**From source** (needs a current stable Rust; libgit2 is vendored and
compiled in, so there is no system dependency beyond OpenSSL on Linux):

```console
$ cargo install --path .        # from a checkout
$ cargo install git-include     # once published to crates.io
```

The binary is named `git-include`, so git automatically picks it up as a
subcommand: `git include <command>`. Verify with:

```console
$ git include --version
```

## Tab completion

Generate a completion script for your shell and source it from your shell
configuration:

```console
# bash — completes both `git-include <TAB>` and `git include <TAB>`,
# including live completion of included directories and branch names
$ git include completions bash > ~/.local/share/bash-completion/completions/git-include

# zsh — place on your $fpath; zsh's git completion dispatches automatically
$ git include completions zsh > ~/.zfunc/_git-include

# fish
$ git include completions fish > ~/.config/fish/completions/git-include.fish
```

Elvish and PowerShell are also supported (`git include completions --help`).

## Quickstart

### Vendor a repository

```console
$ git include add https://github.com/example/widgets vendor/widgets
No branch given; using upstream default branch 'main'.
Fetching https://github.com/example/widgets (main) ...
Added 'vendor/widgets' from https://github.com/example/widgets (branch main, commit 1a2b3c4).
```

This creates **one commit** in your repository containing the full upstream
tree under `vendor/widgets/` plus `vendor/widgets/.gitrepo`. From here on the
directory is completely ordinary: edit it, commit to it, revert it, bisect
through it — it's just files.

### See where you stand

```console
$ git include status --fetch
vendor/widgets
  remote:   https://github.com/example/widgets
  branch:   main (synced at 1a2b3c4)
  upstream: 2 new commit(s) available -> `git include pull vendor/widgets`
  local:    1 commit(s) to push -> `git include push vendor/widgets`

$ git include diff vendor/widgets              # your changes since last sync
$ git include diff vendor/widgets --upstream --fetch   # vs. latest upstream
```

Without `--fetch`, `status` uses the upstream state seen by the most recent
fetch, so it's instant and works offline.

### Pull upstream changes

```console
$ git include pull vendor/widgets
```

Your local changes to the directory (if any) are three-way merged with the
upstream changes, exactly like a `git merge` — including content-level merges
and conflict markers when both sides touched the same lines. The result is a
single commit in your repository. `git include pull --all` syncs every
included directory; with a single include, plain `git include pull` suffices.

When the directory's local state is beyond saving, `git include pull
--force` discards it — committed or not — and takes upstream verbatim.
Force-discarded changes are also excluded from future pushes.

### Push your changes upstream

```console
$ git include push vendor/widgets
Pushed 2 commit(s) from 'vendor/widgets' to https://github.com/example/widgets (main); upstream is now 9f8e7d6.
```

`push` builds a new upstream history: every host commit that changed the
directory since your changes were last incorporated upstream is
cherry-picked onto the upstream branch as its own commit — original message,
original author, but containing only the changes relevant to the directory.
This works **across pulls**: commits you made before pulling upstream
changes still arrive upstream individually (the pull itself contributes
nothing, since its content is already there). The commit hashes necessarily
differ from your host commits, but the content is preserved exactly. The
`.gitrepo` marker is stripped automatically and never appears upstream.

Preview with `git include push -n <dir>`; use `--squash` if you'd rather
publish everything as a single commit. If a commit cannot be cherry-picked
cleanly on its own (e.g. its conflict resolution only exists in a later
merge), push keeps the commits it could replay and combines the remainder
into one final commit, so the pushed content always matches your tree.

If upstream moved in the meantime, `push` refuses and asks you to
`git include pull` first, so upstream never gets surprise merge results.

### Switch the tracked branch

```console
$ git include branches vendor/widgets
* main (1a2b3c4)
  next (5d6e7f8)

$ git include switch vendor/widgets next
Switched 'vendor/widgets' to branch next (commit 5d6e7f8).
```

Local changes are carried over (merged) when switching; a clean directory is
simply swapped to the new branch's content. Switching back is the same
command again. `switch` also accepts a tag or commit id — see
[Pinning to tags and commits](#pinning-to-tags-and-commits).

## Command reference

| Command | Description |
| --- | --- |
| `git include add <remote> <dir> [-b <branch> \| -t <tag> \| --commit <sha>]` | Vendor an upstream repository into `<dir>`, tracking a branch (default: the remote's default branch) or pinned to a tag/commit. |
| `git include pull [<dir>] [--all] [--force]` | Merge new upstream commits into `<dir>` (or all includes); `--force` discards local changes. |
| `git include push <dir> [-n/--dry-run]` | Replay local commits touching `<dir>` onto the upstream branch and push. |
| `git include status [<dir>] [-f/--fetch]` | Show sync state: commits available upstream, commits to push, uncommitted edits. |
| `git include diff <dir> [--upstream] [--stat] [-f/--fetch]` | Diff `<dir>` against the last-synced commit, or against the latest upstream head. |
| `git include switch <dir> <branch\|tag\|commit>` | Track a different branch, or pin to a tag/commit, carrying local changes over. |
| `git include branches <dir>` | List upstream branches and tags, marking the tracked revision. |
| `git include list` | List all includes, nested ones indented. |
| `git include remove <dir>` | Delete an include from the working tree (history and upstream untouched). |
| `git include completions <shell>` | Print a tab-completion script. |
| `git include self-update [--version <tag>]` | Update the git-include binary to the latest (or a specific) release. |

All `<dir>` arguments are relative to your current directory, so the commands
work from anywhere inside the repository. `--no-lfs` is accepted by `add`,
`pull`, `push` and `switch` to skip LFS transfers; `-m/--message` is
accepted by every command that creates a sync commit (see
[Custom commit messages](#custom-commit-messages)).

## Pinning to tags and commits

Unlike git-subrepo, an include does not have to track a branch — it can be
pinned to an exact upstream state:

```console
$ git include add https://github.com/example/widgets vendor/widgets --tag v2.1.0
$ git include add https://github.com/example/parser  vendor/parser  --commit 9f8e7d6c...
$ git include switch vendor/widgets v2.2.0     # move between releases
$ git include switch vendor/widgets main       # back to tracking a branch
```

`switch` resolves its argument automatically (branch first, then tag, then
commit id), so moving between releases and branch-tracking is one command
either way. A pinned include is fully reproducible: `pull` reports the pin
instead of moving, `status`/`diff` compare against the pinned state, and
`push` refuses with a pointer to `switch` (there is no branch to push to).
Local edits are carried over when switching — or discarded with
`switch --force`.

## Custom commit messages

The messages of the sync commits git-include creates (add, pull, switch,
push bookkeeping, init, remove) are templatable with Jinja (via
[minijinja](https://crates.io/crates/minijinja)) — variables, filters and
conditionals all work:

```console
# per repository (or --global), for all sync commits:
$ git config include.commitTemplate 'chore(vendor): {{ action }} {{ subdir }} @ {{ short_commit }}'

# or per invocation:
$ git include pull vendor/widgets -m 'vendor: update widgets to {{ short_commit }}'

# full Jinja expressions are available:
$ git include pull vendor/widgets \
    -m '{% if action == "pull" %}⬆{% endif %} {{ subdir | upper }} @ {{ short_commit }}'
```

| Variable | Value |
| --- | --- |
| `{{ action }}` | `add`, `pull`, `switch`, `push`, `init`, or `remove` |
| `{{ subdir }}` | the included directory |
| `{{ remote }}` | upstream URL |
| `{{ ref }}` (alias `{{ branch }}`) | the tracked branch/tag/commit |
| `{{ commit }}` / `{{ short_commit }}` | the upstream commit (full / 7 chars) |
| `{{ version }}` | the git-include version |

The literal sequence `\n` becomes a newline, so multi-line messages fit in
a single-line config value. A broken template (syntax error or unknown
variable) prints a warning and falls back to the default message — a
finished sync is never aborted over a typo. Without a template,
git-include writes its default structured message
(`git include <action> <dir>` plus a metadata block).

## The `.gitrepo` marker file

Each included directory contains a `.gitrepo` file in git-subrepo's format:

```ini
; DO NOT EDIT (unless you know what you are doing)
;
[subrepo]
	remote = https://github.com/example/widgets
	branch = main
	commit = 1a2b3c4d...   ; upstream commit the directory was last synced to
	parent = 9z8y7x6w...   ; last host commit whose changes are already upstream
	method = merge
	cmdver = 0.1.0
```

Because the format, keys and semantics match git-subrepo, the two tools are
interchangeable: git-include operates on directories vendored with
`git subrepo clone`, and git-subrepo can operate on directories created by
`git include add`. This also means adopting git-include in an existing
git-subrepo project requires no migration at all.

## Git LFS

If the upstream repository uses Git LFS, git-include notices (via
`filter=lfs` in its `.gitattributes`) and handles it automatically:

- **add / pull / switch** fetch the LFS objects from the *upstream* LFS store
  and materialize real content in your working tree,
- **push** uploads LFS objects referenced by your commits *before* pushing the
  git objects, so upstream never sees dangling pointers,
- if `git-lfs` is not installed, operations still succeed — you get pointer
  files plus a clear warning with the exact commands to run later,
- `--no-lfs` skips all of it.

## Exporting a directory into its own repository

The reverse of `add`: a directory that grew inside your repository can
graduate into a repository of its own, history included.

```console
$ git include init mylib --remote git@github.com:me/mylib.git
Extracting the history of 'mylib' ...
Turned 'mylib' into an included repository: extracted 17 commit(s) of history (head 3fc9a21).
Publish it with: git include push mylib

$ git include push mylib
Published 'mylib' to git@github.com:me/mylib.git as new branch 'main'.
```

`init` (alias: `export`) walks your entire history, and every commit that
changed the directory becomes a commit of a brand-new standalone history —
original author and message, content filtered to the directory (a commit
that touched both `mylib/` and other files contributes only its `mylib/`
part). `push` then publishes that history, creating the branch on an empty
remote if needed. From that moment the directory is a normal include:
others can `git include add` it, and `pull`/`push`/`status` work as usual.

## Nested includes

Included repositories can themselves contain includes. Since everything is
plain files, the inner `.gitrepo` markers travel along automatically:

```console
$ git include add https://github.com/example/app libs/app
$ git include list
libs/app  <-  https://github.com/example/app (main @ 4ee9c11)
  libs/app/vendor/parser  <-  https://github.com/example/parser (main @ 77af0d3)
```

You can operate on any level: `git include pull libs/app` syncs the outer
repository (bringing whatever state of `vendor/parser` it has committed),
while `git include pull libs/app/vendor/parser` syncs the inner one directly
from *its* upstream. When pushing an include, only its own marker is stripped —
nested markers are content and are pushed intact.

## Handling merge conflicts

When both you and upstream changed the same lines, `pull` stops with the
conflicting files left in your working tree containing standard conflict
markers:

```console
$ git include pull vendor/widgets
CONFLICT: could not automatically merge upstream changes into 'vendor/widgets'.
Files with conflict markers:
  vendor/widgets/src/lib.rs

Resolve the conflicts, then finish with:
  git add vendor/widgets
  git commit
```

There is no special "continue" state to manage: resolve the markers, `git add`,
`git commit` — done. (The `.gitrepo` update is already staged for you.) If you
want to bail out instead, `git reset --hard` restores the pre-pull state.

## How it works

Every operation is a pure function of the four marker values (`remote`,
`branch`, `commit`, `parent`) plus the current state of the host repository
and the upstream remote — there is no state in `.git/config`, no registered
remotes, no temporary branches. All of it runs in-process through libgit2
(the `git2` crate):

- `add` fetches the upstream branch and grafts its tree under the prefix by
  rewriting the root tree — one commit, no shared history with upstream.
- `pull` takes three trees — the last-synced upstream commit's tree (base),
  your current directory tree (ours), and the new upstream head's tree
  (theirs) — and hands them to libgit2's three-way merge (rename detection
  included). A clean merge becomes a single host commit; conflicts are
  materialized in your working tree with standard conflict markers.
- `push` first verifies the upstream branch still points at the recorded
  base (so the result is a pure fast-forward upstream, never a surprise
  merge), then replays each host commit that changed the directory as a new
  commit on top of the upstream branch — subdirectory tree with the marker
  stripped, original message and author preserved. Marker-only bookkeeping
  commits are skipped automatically. Only the include's *own* marker is
  stripped; nested `.gitrepo` files are content and travel upstream intact.
- Fetched upstream heads are pinned under `refs/include/<dir>` so `status`
  and `diff` work offline and fetched objects survive `git gc`.

One subtle case is handled explicitly: a fresh clone of the host repository
has the vendored *trees and blobs* (they're reachable from host commits)
but not the upstream *commit* objects. Syncing commands therefore re-fetch
from the upstream remote on demand, and detect upstream history rewrites
(force-pushes) with a clear recovery path instead of producing a bogus
merge.

No temporary branches, no `.git/modules`, no stashing, no touching your
working tree outside the included directory — and unlike git-subrepo, no
dependency on `git subtree`-style squash-merge machinery.

## FAQ

**Do my collaborators need git-include?**
No. The vendored directory is regular files. Only whoever runs
`pull`/`push`/`switch` needs the tool.

**Does `add` bloat my repository?**
You get the upstream *tree* (one snapshot), not its history, in your branch.
The fetched upstream history stays in your local object store for merging but
is never pushed to your host remote.

**Can I edit vendored files directly?**
Yes — that's the point. Commit as usual; `git include status` shows what
hasn't been pushed upstream yet.

**What if upstream force-pushed?**
`pull` and `push` detect that the recorded commit no longer exists upstream
and tell you how to recover.

**Which git version do I need?**
None at runtime — git-include embeds libgit2 and talks to remotes itself.
The only optional external dependency is `git-lfs` (with git) for LFS
content, and your credentials are picked up the standard way (ssh-agent and
git credential helpers).

## License

MIT — see [LICENSE](LICENSE).
