//! End-to-end tests: every scenario builds real git repositories in a temp
//! directory and drives the compiled `git-include` binary, asserting on the
//! resulting git state (commits, trees, marker files, upstream content).

mod common;

use common::*;

// ---------------------------------------------------------------- add ----

#[test]
fn add_includes_upstream_files_and_writes_compatible_marker() {
    let env = TestEnv::new();
    let (url, up_work) = env.upstream("lib");
    upstream_commit(&up_work, "src/lib.rs", "pub fn hello() {}\n", "add lib.rs");
    let host = env.work_repo("host");

    let out = include_ok(&host, &["add", &url, "vendor/lib", "--branch", "main"]);
    assert!(
        out.contains("Added 'vendor/lib'"),
        "unexpected output: {out}"
    );

    // Files are plain files in the host repo.
    assert_eq!(read(&host, "vendor/lib/src/lib.rs"), "pub fn hello() {}\n");
    assert_clean(&host);

    // Marker file is git-subrepo compatible: [subrepo] section, tab-indented
    // keys, correct values.
    let marker = read(&host, "vendor/lib/.gitrepo");
    assert!(marker.contains("[subrepo]"), "marker:\n{marker}");
    assert!(
        marker.contains(&format!("\tremote = {url}")),
        "marker:\n{marker}"
    );
    assert!(marker.contains("\tbranch = main"), "marker:\n{marker}");
    let upstream_sha = git_in(&up_work, &["rev-parse", "origin/main"]);
    assert!(
        marker.contains(&format!("\tcommit = {upstream_sha}")),
        "marker:\n{marker}"
    );
    assert!(marker.contains("\tparent = "), "marker:\n{marker}");
    assert!(marker.contains("\tmethod = merge"), "marker:\n{marker}");
    assert!(marker.contains("\tcmdver = "), "marker:\n{marker}");

    // git-subrepo itself parses the marker with `git config --file`; make
    // sure that works too.
    let remote = git_in(
        &host,
        &["config", "--file", "vendor/lib/.gitrepo", "subrepo.remote"],
    );
    assert_eq!(remote, url);

    // Exactly one new commit on the host.
    let count = git_in(&host, &["rev-list", "--count", "HEAD"]);
    assert_eq!(count, "2"); // initial + add
}

#[test]
fn add_uses_remote_default_branch_when_none_given() {
    let env = TestEnv::new();
    let (url, up_work) = env.upstream("lib");
    upstream_commit(&up_work, "a.txt", "a\n", "add a");
    let host = env.work_repo("host");

    include_ok(&host, &["add", &url, "vendor/lib"]);
    let branch = git_in(
        &host,
        &["config", "--file", "vendor/lib/.gitrepo", "subrepo.branch"],
    );
    assert_eq!(branch, "main");
}

#[test]
fn add_refuses_dirty_worktree() {
    let env = TestEnv::new();
    let (url, _up) = env.upstream("lib");
    let host = env.work_repo("host");
    std::fs::write(host.join("README.md"), "dirty\n").unwrap();

    let err = include_err(&host, &["add", &url, "vendor/lib"]);
    assert!(err.contains("uncommitted changes"), "got: {err}");
}

#[test]
fn add_refuses_existing_directory_and_double_add() {
    let env = TestEnv::new();
    let (url, _up) = env.upstream("lib");
    let host = env.work_repo("host");
    commit_file(&host, "vendor/lib/existing.txt", "x\n", "existing dir");

    let err = include_err(&host, &["add", &url, "vendor/lib"]);
    assert!(err.contains("already"), "got: {err}");
}

// --------------------------------------------------------------- pull ----

#[test]
fn pull_is_noop_when_up_to_date() {
    let env = TestEnv::new();
    let (url, _up) = env.upstream("lib");
    let host = env.work_repo("host");
    include_ok(&host, &["add", &url, "vendor/lib"]);

    let head = git_in(&host, &["rev-parse", "HEAD"]);
    let out = include_ok(&host, &["pull", "vendor/lib"]);
    assert!(out.contains("up to date"), "got: {out}");
    assert_eq!(
        git_in(&host, &["rev-parse", "HEAD"]),
        head,
        "no commit expected"
    );
}

#[test]
fn pull_brings_in_new_upstream_commits() {
    let env = TestEnv::new();
    let (url, up_work) = env.upstream("lib");
    let host = env.work_repo("host");
    include_ok(&host, &["add", &url, "vendor/lib"]);

    upstream_commit(&up_work, "new.txt", "fresh\n", "upstream adds new.txt");
    include_ok(&host, &["pull", "vendor/lib"]);

    assert_eq!(read(&host, "vendor/lib/new.txt"), "fresh\n");
    assert_clean(&host);
    let sha = git_in(&up_work, &["rev-parse", "origin/main"]);
    let recorded = git_in(
        &host,
        &["config", "--file", "vendor/lib/.gitrepo", "subrepo.commit"],
    );
    assert_eq!(recorded, sha, "marker must track the new upstream commit");
}

#[test]
fn pull_merges_upstream_with_local_changes() {
    let env = TestEnv::new();
    let (url, up_work) = env.upstream("lib");
    let host = env.work_repo("host");
    include_ok(&host, &["add", &url, "vendor/lib"]);

    // Local commit inside the include; upstream commits a different file.
    commit_file(
        &host,
        "vendor/lib/local.txt",
        "local\n",
        "host: local addition",
    );
    upstream_commit(&up_work, "upstream.txt", "up\n", "upstream addition");

    include_ok(&host, &["pull", "vendor/lib"]);
    assert_eq!(read(&host, "vendor/lib/local.txt"), "local\n");
    assert_eq!(read(&host, "vendor/lib/upstream.txt"), "up\n");
    assert_clean(&host);
}

#[test]
fn pull_conflict_leaves_markers_and_resolution_flow_works() {
    let env = TestEnv::new();
    let (url, up_work) = env.upstream("lib");
    upstream_commit(&up_work, "shared.txt", "original\n", "add shared");
    let host = env.work_repo("host");
    include_ok(&host, &["add", &url, "vendor/lib"]);

    commit_file(
        &host,
        "vendor/lib/shared.txt",
        "host version\n",
        "host edit",
    );
    upstream_commit(
        &up_work,
        "shared.txt",
        "upstream version\n",
        "upstream edit",
    );

    let err = include_err(&host, &["pull", "vendor/lib"]);
    assert!(err.contains("CONFLICT"), "got: {err}");
    assert!(err.contains("vendor/lib/shared.txt"), "got: {err}");

    let conflicted = read(&host, "vendor/lib/shared.txt");
    assert!(
        conflicted.contains("<<<<<<<"),
        "expected conflict markers:\n{conflicted}"
    );
    assert!(conflicted.contains("host version"), "{conflicted}");
    assert!(conflicted.contains("upstream version"), "{conflicted}");

    // The marker file was already advanced to the new upstream commit.
    let sha = git_in(&up_work, &["rev-parse", "origin/main"]);
    let recorded = git_in(
        &host,
        &["config", "--file", "vendor/lib/.gitrepo", "subrepo.commit"],
    );
    assert_eq!(recorded, sha);

    // Resolve exactly as the error message instructs.
    std::fs::write(host.join("vendor/lib/shared.txt"), "resolved\n").unwrap();
    git_in(&host, &["add", "vendor/lib"]);
    git_in(
        &host,
        &["commit", "-q", "-m", "merge upstream into vendor/lib"],
    );
    assert_clean(&host);

    // And the resolution can be pushed upstream.
    include_ok(&host, &["push", "vendor/lib"]);
    let up_clone = env.path("up-check");
    git_in(
        env.root.path(),
        &["clone", "-q", &url, up_clone.to_str().unwrap()],
    );
    assert_eq!(read(&up_clone, "shared.txt"), "resolved\n");
}

#[test]
fn pull_without_argument_targets_single_include_and_all_flag_works() {
    let env = TestEnv::new();
    let (url_a, up_a) = env.upstream("liba");
    let (url_b, up_b) = env.upstream("libb");
    let host = env.work_repo("host");
    include_ok(&host, &["add", &url_a, "vendor/a"]);

    upstream_commit(&up_a, "fa.txt", "a\n", "a update");
    include_ok(&host, &["pull"]); // no argument: only one include
    assert_eq!(read(&host, "vendor/a/fa.txt"), "a\n");

    include_ok(&host, &["add", &url_b, "vendor/b"]);
    let err = include_err(&host, &["pull"]); // ambiguous now
    assert!(err.contains("--all"), "got: {err}");

    upstream_commit(&up_a, "fa2.txt", "a2\n", "a update 2");
    upstream_commit(&up_b, "fb.txt", "b\n", "b update");
    include_ok(&host, &["pull", "--all"]);
    assert_eq!(read(&host, "vendor/a/fa2.txt"), "a2\n");
    assert_eq!(read(&host, "vendor/b/fb.txt"), "b\n");
    assert_clean(&host);
}

// --------------------------------------------------------------- push ----

#[test]
fn push_replays_local_commits_upstream_without_marker() {
    let env = TestEnv::new();
    let (url, _up) = env.upstream("lib");
    let host = env.work_repo("host");
    include_ok(&host, &["add", &url, "vendor/lib"]);

    commit_file(&host, "vendor/lib/feature.txt", "v1\n", "add feature file");
    commit_file(
        &host,
        "vendor/lib/feature.txt",
        "v2\n",
        "improve feature file",
    );

    let out = include_ok(&host, &["push", "vendor/lib"]);
    assert!(out.contains("Pushed 2 commit(s)"), "got: {out}");
    assert_clean(&host);

    let clone = env.path("check");
    git_in(
        env.root.path(),
        &["clone", "-q", &url, clone.to_str().unwrap()],
    );
    assert_eq!(read(&clone, "feature.txt"), "v2\n");
    // Individual commits and messages are preserved...
    let log = git_in(&clone, &["log", "--format=%s", "main"]);
    assert!(log.contains("add feature file"), "log: {log}");
    assert!(log.contains("improve feature file"), "log: {log}");
    // ...and the marker file never leaks upstream.
    assert!(
        !clone.join(".gitrepo").exists(),
        ".gitrepo must not be pushed upstream"
    );

    // The marker now records the new upstream head; a follow-up pull is a
    // no-op and status is clean.
    let out = include_ok(&host, &["pull", "vendor/lib"]);
    assert!(out.contains("up to date"), "got: {out}");
    let status = include_ok(&host, &["status", "vendor/lib"]);
    assert!(status.contains("up to date"), "got: {status}");
    assert!(status.contains("clean"), "got: {status}");
}

#[test]
fn push_preserves_commit_authors() {
    let env = TestEnv::new();
    let (url, _up) = env.upstream("lib");
    let host = env.work_repo("host");
    include_ok(&host, &["add", &url, "vendor/lib"]);

    std::fs::write(host.join("vendor/lib/authored.txt"), "hi\n").unwrap();
    git_in(&host, &["add", "vendor/lib/authored.txt"]);
    git_in(
        &host,
        &[
            "-c",
            "user.name=Alice Author",
            "-c",
            "user.email=alice@example.com",
            "commit",
            "-q",
            "-m",
            "authored change",
        ],
    );

    include_ok(&host, &["push", "vendor/lib"]);
    let clone = env.path("check");
    git_in(
        env.root.path(),
        &["clone", "-q", &url, clone.to_str().unwrap()],
    );
    let author = git_in(&clone, &["log", "-1", "--format=%an <%ae>", "main"]);
    assert_eq!(author, "Alice Author <alice@example.com>");
}

#[test]
fn push_with_nothing_to_push_is_a_noop() {
    let env = TestEnv::new();
    let (url, _up) = env.upstream("lib");
    let host = env.work_repo("host");
    include_ok(&host, &["add", &url, "vendor/lib"]);

    let head = git_in(&host, &["rev-parse", "HEAD"]);
    let out = include_ok(&host, &["push", "vendor/lib"]);
    assert!(out.contains("no local changes"), "got: {out}");
    assert_eq!(git_in(&host, &["rev-parse", "HEAD"]), head);
}

#[test]
fn push_requires_pull_when_upstream_moved() {
    let env = TestEnv::new();
    let (url, up_work) = env.upstream("lib");
    let host = env.work_repo("host");
    include_ok(&host, &["add", &url, "vendor/lib"]);

    commit_file(&host, "vendor/lib/local.txt", "l\n", "local change");
    upstream_commit(&up_work, "remote.txt", "r\n", "remote change");

    let err = include_err(&host, &["push", "vendor/lib"]);
    assert!(err.contains("git include pull"), "got: {err}");

    // After pulling, the push goes through — and the local commit arrives
    // upstream as its own commit, not folded into the pull.
    include_ok(&host, &["pull", "vendor/lib"]);
    include_ok(&host, &["push", "vendor/lib"]);
    let clone = env.path("check");
    git_in(
        env.root.path(),
        &["clone", "-q", &url, clone.to_str().unwrap()],
    );
    assert_eq!(read(&clone, "local.txt"), "l\n");
    assert_eq!(read(&clone, "remote.txt"), "r\n");
    let log = git_in(&clone, &["log", "--format=%s", "main"]);
    assert!(log.contains("local change"), "log: {log}");
    assert!(!log.contains("git include pull"), "log: {log}");
}

#[test]
fn push_preserves_individual_commits_across_pulls() {
    let env = TestEnv::new();
    let (url, up_work) = env.upstream("lib");
    let host = env.work_repo("host");
    include_ok(&host, &["add", &url, "vendor/lib"]);

    // Two local commits, then upstream moves, then a pull — the classic
    // situation where naive tools squash everything into the pull commit.
    commit_file(&host, "vendor/lib/one.txt", "1\n", "first local commit");
    commit_file(&host, "vendor/lib/two.txt", "2\n", "second local commit");
    upstream_commit(&up_work, "upstream.txt", "u\n", "upstream work");
    include_ok(&host, &["pull", "vendor/lib"]);

    // status counts the pre-pull commits as still unpushed.
    let s = include_ok(&host, &["status", "vendor/lib"]);
    assert!(s.contains("2 commit(s) to push"), "got: {s}");

    include_ok(&host, &["push", "vendor/lib"]);
    let clone = env.path("check");
    git_in(
        env.root.path(),
        &["clone", "-q", &url, clone.to_str().unwrap()],
    );
    assert_eq!(read(&clone, "one.txt"), "1\n");
    assert_eq!(read(&clone, "two.txt"), "2\n");
    assert_eq!(read(&clone, "upstream.txt"), "u\n");
    // Both local commits exist individually upstream (new hashes, same
    // messages); the pull left no trace in upstream history.
    let log = git_in(&clone, &["log", "--format=%s", "main"]);
    assert!(log.contains("first local commit"), "log: {log}");
    assert!(log.contains("second local commit"), "log: {log}");
    assert!(!log.contains("git include"), "log: {log}");
    // And afterwards everything is in sync.
    let s = include_ok(&host, &["status", "vendor/lib"]);
    assert!(s.contains("up to date"), "got: {s}");
    assert!(s.contains("clean"), "got: {s}");
}

#[test]
fn push_squash_flattens_local_commits() {
    let env = TestEnv::new();
    let (url, _up) = env.upstream("lib");
    let host = env.work_repo("host");
    include_ok(&host, &["add", &url, "vendor/lib"]);

    commit_file(&host, "vendor/lib/a.txt", "a\n", "step one");
    commit_file(&host, "vendor/lib/b.txt", "b\n", "step two");

    let out = include_ok(&host, &["push", "vendor/lib", "--squash"]);
    assert!(out.contains("Pushed 1 commit(s)"), "got: {out}");
    let clone = env.path("check");
    git_in(
        env.root.path(),
        &["clone", "-q", &url, clone.to_str().unwrap()],
    );
    assert_eq!(read(&clone, "a.txt"), "a\n");
    assert_eq!(read(&clone, "b.txt"), "b\n");
    // One squashed commit on top of the initial upstream commit.
    assert_eq!(git_in(&clone, &["rev-list", "--count", "main"]), "2");
    let body = git_in(&clone, &["log", "-1", "--format=%B", "main"]);
    assert!(body.contains("step one"), "body: {body}");
    assert!(body.contains("step two"), "body: {body}");
}

#[test]
fn push_dry_run_pushes_nothing() {
    let env = TestEnv::new();
    let (url, up_work) = env.upstream("lib");
    let host = env.work_repo("host");
    include_ok(&host, &["add", &url, "vendor/lib"]);
    commit_file(&host, "vendor/lib/f.txt", "x\n", "change");

    let before = git_in(&up_work, &["ls-remote", "origin", "main"]);
    let out = include_ok(&host, &["push", "vendor/lib", "--dry-run"]);
    assert!(out.contains("would push"), "got: {out}");
    let after = git_in(&up_work, &["ls-remote", "origin", "main"]);
    assert_eq!(before, after, "dry run must not move upstream");
}

#[test]
fn push_refuses_to_delete_upstream() {
    let env = TestEnv::new();
    let (url, _up) = env.upstream("lib");
    let host = env.work_repo("host");
    include_ok(&host, &["add", &url, "vendor/lib"]);

    git_in(&host, &["rm", "-r", "-q", "vendor/lib"]);
    git_in(&host, &["commit", "-q", "-m", "drop vendor/lib"]);
    // Restore it so the marker exists again, with history containing the
    // deletion in between.
    git_in(&host, &["revert", "--no-edit", "HEAD"]);

    let err = include_err(&host, &["push", "vendor/lib"]);
    assert!(err.contains("refusing to push a deletion"), "got: {err}");
}

// ------------------------------------------------------------- status ----

#[test]
fn status_reports_behind_ahead_and_dirty() {
    let env = TestEnv::new();
    let (url, up_work) = env.upstream("lib");
    let host = env.work_repo("host");
    include_ok(&host, &["add", &url, "vendor/lib"]);

    // Freshly added: everything clean.
    let s = include_ok(&host, &["status"]);
    assert!(s.contains("vendor/lib"), "got: {s}");
    assert!(s.contains("upstream: up to date"), "got: {s}");
    assert!(s.contains("local:    clean"), "got: {s}");

    // Upstream moves: --fetch sees it.
    upstream_commit(&up_work, "u1.txt", "1\n", "u1");
    upstream_commit(&up_work, "u2.txt", "2\n", "u2");
    let s = include_ok(&host, &["status", "vendor/lib", "--fetch"]);
    assert!(s.contains("2 new commit(s)"), "got: {s}");

    // Local commits to push are counted.
    commit_file(&host, "vendor/lib/l.txt", "l\n", "local");
    let s = include_ok(&host, &["status", "vendor/lib"]);
    assert!(s.contains("1 commit(s) to push"), "got: {s}");

    // Uncommitted edits are flagged separately.
    std::fs::write(host.join("vendor/lib/l.txt"), "edited\n").unwrap();
    let s = include_ok(&host, &["status", "vendor/lib"]);
    assert!(s.contains("uncommitted changes"), "got: {s}");
}

// --------------------------------------------------------------- diff ----

#[test]
fn diff_shows_local_changes_and_upstream_changes() {
    let env = TestEnv::new();
    let (url, up_work) = env.upstream("lib");
    upstream_commit(&up_work, "code.txt", "line1\n", "base");
    let host = env.work_repo("host");
    include_ok(&host, &["add", &url, "vendor/lib"]);

    // No changes yet.
    let d = include_ok(&host, &["diff", "vendor/lib"]);
    assert!(d.contains("no local changes"), "got: {d}");

    // Local edit shows up (paths are subdir-relative, marker excluded).
    commit_file(&host, "vendor/lib/code.txt", "line1\nline2\n", "local edit");
    let d = include_ok(&host, &["diff", "vendor/lib"]);
    assert!(d.contains("+line2"), "got: {d}");
    assert!(
        !d.contains(".gitrepo"),
        "marker must not appear in diffs: {d}"
    );

    // Upstream comparison after fetch.
    upstream_commit(&up_work, "up-only.txt", "up\n", "upstream file");
    let d = include_ok(&host, &["diff", "vendor/lib", "--upstream", "--fetch"]);
    assert!(d.contains("up-only.txt"), "got: {d}");

    let d = include_ok(&host, &["diff", "vendor/lib", "--stat"]);
    assert!(d.contains("code.txt"), "got: {d}");
}

// ----------------------------------------------------- branch switching ----

#[test]
fn branches_lists_upstream_branches_with_tracked_marker() {
    let env = TestEnv::new();
    let (url, up_work) = env.upstream("lib");
    git_in(&up_work, &["checkout", "-q", "-b", "dev"]);
    upstream_commit(&up_work, "dev.txt", "dev\n", "dev work");
    git_in(&up_work, &["checkout", "-q", "main"]);

    let host = env.work_repo("host");
    include_ok(&host, &["add", &url, "vendor/lib", "--branch", "main"]);

    let out = include_ok(&host, &["branches", "vendor/lib"]);
    assert!(out.contains("* main"), "got: {out}");
    assert!(out.contains("  dev"), "got: {out}");
}

#[test]
fn switch_moves_to_another_branch_and_back() {
    let env = TestEnv::new();
    let (url, up_work) = env.upstream("lib");
    upstream_commit(&up_work, "common.txt", "common\n", "common");
    git_in(&up_work, &["checkout", "-q", "-b", "dev"]);
    upstream_commit(&up_work, "dev-only.txt", "dev\n", "dev feature");
    git_in(&up_work, &["checkout", "-q", "main"]);

    let host = env.work_repo("host");
    include_ok(&host, &["add", &url, "vendor/lib", "--branch", "main"]);
    assert!(!host.join("vendor/lib/dev-only.txt").exists());

    include_ok(&host, &["switch", "vendor/lib", "dev"]);
    assert_eq!(read(&host, "vendor/lib/dev-only.txt"), "dev\n");
    assert_eq!(read(&host, "vendor/lib/common.txt"), "common\n");
    let branch = git_in(
        &host,
        &["config", "--file", "vendor/lib/.gitrepo", "subrepo.branch"],
    );
    assert_eq!(branch, "dev");
    assert_clean(&host);

    // Switching back removes branch-only files again.
    include_ok(&host, &["switch", "vendor/lib", "main"]);
    assert!(!host.join("vendor/lib/dev-only.txt").exists());
    assert_eq!(read(&host, "vendor/lib/common.txt"), "common\n");
    assert_clean(&host);
}

#[test]
fn switch_carries_local_changes_over() {
    let env = TestEnv::new();
    let (url, up_work) = env.upstream("lib");
    git_in(&up_work, &["checkout", "-q", "-b", "dev"]);
    upstream_commit(&up_work, "dev.txt", "dev\n", "dev");
    git_in(&up_work, &["checkout", "-q", "main"]);

    let host = env.work_repo("host");
    include_ok(&host, &["add", &url, "vendor/lib", "--branch", "main"]);
    commit_file(&host, "vendor/lib/local.txt", "keep me\n", "local work");

    include_ok(&host, &["switch", "vendor/lib", "dev"]);
    assert_eq!(read(&host, "vendor/lib/local.txt"), "keep me\n");
    assert_eq!(read(&host, "vendor/lib/dev.txt"), "dev\n");
    assert_clean(&host);
}

#[test]
fn switch_to_same_branch_is_a_noop() {
    let env = TestEnv::new();
    let (url, _up) = env.upstream("lib");
    let host = env.work_repo("host");
    include_ok(&host, &["add", &url, "vendor/lib"]);
    let out = include_ok(&host, &["switch", "vendor/lib", "main"]);
    assert!(out.contains("already tracks"), "got: {out}");
}

// ------------------------------------------------------------ nesting ----

#[test]
fn nested_includes_survive_all_operations() {
    let env = TestEnv::new();
    let (url_b, up_b) = env.upstream("libb");
    upstream_commit(&up_b, "b.txt", "b1\n", "b content");

    // Repo A itself includes B (nesting level 1).
    let (url_a, work_a) = env.upstream("liba");
    include_ok(&work_a, &["add", &url_b, "vendor/b"]);
    git_in(&work_a, &["push", "-q", "origin", "main"]);

    // Host includes A; B arrives nested inside it.
    let host = env.work_repo("host");
    include_ok(&host, &["add", &url_a, "libs/a"]);
    assert_eq!(read(&host, "libs/a/vendor/b/b.txt"), "b1\n");
    assert!(host.join("libs/a/vendor/b/.gitrepo").exists());

    // list shows both, nested one indented under its parent.
    let out = include_ok(&host, &["list"]);
    assert!(out.contains("libs/a  <-"), "got: {out}");
    assert!(out.contains("  libs/a/vendor/b  <-"), "got: {out}");

    // status handles the nested include (whose upstream commit is not in
    // the host object store) without crashing.
    let s = include_ok(&host, &["status"]);
    assert!(s.contains("libs/a/vendor/b"), "got: {s}");

    // B updates; A pulls it and publishes; host pulls A -> nested content
    // and nested marker update flow through.
    upstream_commit(&up_b, "b.txt", "b2\n", "b update");
    include_ok(&work_a, &["pull", "vendor/b"]);
    git_in(&work_a, &["push", "-q", "origin", "main"]);
    include_ok(&host, &["pull", "libs/a"]);
    assert_eq!(read(&host, "libs/a/vendor/b/b.txt"), "b2\n");

    // Host edits inside A (outside B) and pushes to A: A's marker is
    // stripped but B's nested marker must be preserved upstream.
    commit_file(
        &host,
        "libs/a/host-feature.txt",
        "hf\n",
        "host feature for A",
    );
    include_ok(&host, &["push", "libs/a"]);
    let clone = env.path("a-check");
    git_in(
        env.root.path(),
        &["clone", "-q", &url_a, clone.to_str().unwrap()],
    );
    assert_eq!(read(&clone, "host-feature.txt"), "hf\n");
    assert!(
        !clone.join(".gitrepo").exists(),
        "A's own marker must be stripped"
    );
    assert!(
        clone.join("vendor/b/.gitrepo").exists(),
        "nested marker must survive"
    );

    // The pushed-to clone of A is itself fully operational: pull B there.
    configure_user(&clone);
    let out = include_ok(&clone, &["pull", "vendor/b"]);
    assert!(out.contains("up to date"), "got: {out}");
}

// ------------------------------------------------------- collaboration ----

#[test]
fn fresh_clone_of_host_can_pull_and_push() {
    let env = TestEnv::new();
    let (url, up_work) = env.upstream("lib");
    let (host_url, host_work) = env.upstream("host");
    include_ok(&host_work, &["add", &url, "vendor/lib"]);
    git_in(&host_work, &["push", "-q", "origin", "main"]);

    // A collaborator clones the host repo. The upstream *commit* objects
    // are not reachable from host history, only the trees/blobs are — the
    // tool must recover by fetching.
    let clone = env.path("collab");
    git_in(
        env.root.path(),
        &["clone", "-q", &host_url, clone.to_str().unwrap()],
    );
    configure_user(&clone);
    assert_eq!(read(&clone, "vendor/lib/README.md"), "# lib-work\n");

    upstream_commit(&up_work, "from-upstream.txt", "u\n", "upstream work");
    include_ok(&clone, &["pull", "vendor/lib"]);
    assert_eq!(read(&clone, "vendor/lib/from-upstream.txt"), "u\n");

    commit_file(&clone, "vendor/lib/from-collab.txt", "c\n", "collab work");
    include_ok(&clone, &["push", "vendor/lib"]);
    let check = env.path("check");
    git_in(
        env.root.path(),
        &["clone", "-q", &url, check.to_str().unwrap()],
    );
    assert_eq!(read(&check, "from-collab.txt"), "c\n");
}

// ------------------------------------------------- subrepo compatibility ----

#[test]
fn operates_on_marker_written_by_git_subrepo() {
    let env = TestEnv::new();
    let (url, up_work) = env.upstream("lib");
    let host = env.work_repo("host");
    include_ok(&host, &["add", &url, "vendor/lib"]);

    // Rewrite the marker exactly as git-subrepo 0.4.9 would have written
    // it (its header, its cmdver), simulating a repo previously managed by
    // git-subrepo.
    let commit = git_in(
        &host,
        &["config", "--file", "vendor/lib/.gitrepo", "subrepo.commit"],
    );
    let parent = git_in(
        &host,
        &["config", "--file", "vendor/lib/.gitrepo", "subrepo.parent"],
    );
    let subrepo_style = format!(
        "; DO NOT EDIT (unless you know what you are doing)\n\
         ;\n\
         ; This subdirectory is a git \"subrepo\", and this file is maintained by the\n\
         ; git-subrepo command. See https://github.com/ingydotnet/git-subrepo#readme\n\
         ;\n\
         [subrepo]\n\
         \tremote = {url}\n\
         \tbranch = main\n\
         \tcommit = {commit}\n\
         \tparent = {parent}\n\
         \tmethod = merge\n\
         \tcmdver = 0.4.9\n"
    );
    std::fs::write(host.join("vendor/lib/.gitrepo"), subrepo_style).unwrap();
    git_in(
        &host,
        &["commit", "-q", "-am", "convert marker to git-subrepo style"],
    );

    // status, pull and push all work on the subrepo-written marker.
    let s = include_ok(&host, &["status", "vendor/lib"]);
    assert!(s.contains(&url), "got: {s}");

    upstream_commit(&up_work, "next.txt", "n\n", "upstream next");
    include_ok(&host, &["pull", "vendor/lib"]);
    assert_eq!(read(&host, "vendor/lib/next.txt"), "n\n");

    commit_file(&host, "vendor/lib/ours.txt", "o\n", "our change");
    include_ok(&host, &["push", "vendor/lib"]);
    let clone = env.path("check");
    git_in(
        env.root.path(),
        &["clone", "-q", &url, clone.to_str().unwrap()],
    );
    assert_eq!(read(&clone, "ours.txt"), "o\n");
}

#[test]
fn commit_touching_two_includes_pushes_only_relevant_changes() {
    let env = TestEnv::new();
    let (url_a, _up_a) = env.upstream("liba");
    let (url_b, _up_b) = env.upstream("libb");
    let host = env.work_repo("host");
    include_ok(&host, &["add", &url_a, "vendor/a"]);
    include_ok(&host, &["add", &url_b, "vendor/b"]);

    // One host commit spanning both includes (plus an unrelated file).
    std::fs::write(host.join("vendor/a/from-host.txt"), "for a\n").unwrap();
    std::fs::write(host.join("vendor/b/from-host.txt"), "for b\n").unwrap();
    std::fs::write(host.join("unrelated.txt"), "x\n").unwrap();
    git_in(&host, &["add", "."]);
    git_in(&host, &["commit", "-q", "-m", "update both vendored libs"]);

    include_ok(&host, &["push", "vendor/a"]);
    include_ok(&host, &["push", "vendor/b"]);

    for (url, content) in [(&url_a, "for a\n"), (&url_b, "for b\n")] {
        let clone = env.path(&format!("check-{content}"));
        git_in(
            env.root.path(),
            &["clone", "-q", url, clone.to_str().unwrap()],
        );
        assert_eq!(read(&clone, "from-host.txt"), content);
        assert!(!clone.join("unrelated.txt").exists());
        let log = git_in(&clone, &["log", "--format=%s", "main"]);
        assert!(log.contains("update both vendored libs"), "log: {log}");
    }

    // Both includes are fully synced afterwards.
    let s = include_ok(&host, &["status"]);
    assert_eq!(s.matches("clean").count(), 2, "got: {s}");
}

#[test]
fn push_after_switch_lands_commits_on_the_new_branch() {
    let env = TestEnv::new();
    let (url, up_work) = env.upstream("lib");
    git_in(&up_work, &["checkout", "-q", "-b", "dev"]);
    upstream_commit(&up_work, "dev.txt", "dev\n", "dev base");
    git_in(&up_work, &["checkout", "-q", "main"]);

    let host = env.work_repo("host");
    include_ok(&host, &["add", &url, "vendor/lib", "--branch", "main"]);
    commit_file(
        &host,
        "vendor/lib/feature.txt",
        "f\n",
        "feature while on main",
    );
    include_ok(&host, &["switch", "vendor/lib", "dev"]);
    include_ok(&host, &["push", "vendor/lib"]);

    // The commit made while tracking main was carried over by the switch
    // and pushed to dev; main is untouched.
    let clone = env.path("check");
    git_in(
        env.root.path(),
        &["clone", "-q", "-b", "dev", &url, clone.to_str().unwrap()],
    );
    assert_eq!(read(&clone, "feature.txt"), "f\n");
    assert_eq!(read(&clone, "dev.txt"), "dev\n");
    let log = git_in(&clone, &["log", "--format=%s", "dev"]);
    assert!(log.contains("feature while on main"), "log: {log}");
    let main_log = git_in(&clone, &["log", "--format=%s", "origin/main"]);
    assert!(!main_log.contains("feature"), "main log: {main_log}");
}

#[test]
fn marker_without_parent_entry_still_pulls_and_pushes() {
    let env = TestEnv::new();
    let (url, up_work) = env.upstream("lib");
    let host = env.work_repo("host");
    include_ok(&host, &["add", &url, "vendor/lib"]);

    // git-subrepo tolerates markers without a parent line (e.g. written
    // by hand); strip it and make sure we degrade gracefully.
    let commit = git_in(
        &host,
        &["config", "--file", "vendor/lib/.gitrepo", "subrepo.commit"],
    );
    std::fs::write(
        host.join("vendor/lib/.gitrepo"),
        format!("[subrepo]\n\tremote = {url}\n\tbranch = main\n\tcommit = {commit}\n"),
    )
    .unwrap();
    git_in(&host, &["commit", "-q", "-am", "strip parent from marker"]);

    // push without a parent is refused with a helpful message ...
    let err = include_err(&host, &["push", "vendor/lib"]);
    assert!(err.contains("parent"), "got: {err}");

    // ... and a pull bootstraps the parent, after which push works.
    upstream_commit(&up_work, "u.txt", "u\n", "upstream work");
    include_ok(&host, &["pull", "vendor/lib"]);
    let parent = git_in(
        &host,
        &["config", "--file", "vendor/lib/.gitrepo", "subrepo.parent"],
    );
    assert!(!parent.is_empty(), "pull must bootstrap the parent entry");

    commit_file(&host, "vendor/lib/mine.txt", "m\n", "my change");
    include_ok(&host, &["push", "vendor/lib"]);
    let clone = env.path("check");
    git_in(
        env.root.path(),
        &["clone", "-q", &url, clone.to_str().unwrap()],
    );
    assert_eq!(read(&clone, "mine.txt"), "m\n");
}

// ------------------------------------------------- tag / commit pinning ----

#[test]
fn add_pinned_to_tag_or_commit_and_push_is_refused() {
    let env = TestEnv::new();
    let (url, up_work) = env.upstream("lib");
    upstream_commit(&up_work, "f.txt", "v1\n", "version 1");
    git_in(&up_work, &["tag", "-a", "v1.0", "-m", "release v1.0"]);
    git_in(&up_work, &["push", "-q", "origin", "v1.0"]);
    let pinned_sha = git_in(&up_work, &["rev-parse", "HEAD"]);
    upstream_commit(&up_work, "f.txt", "v2\n", "version 2 (after the tag)");

    let host = env.work_repo("host");

    // Pin to the annotated tag: content is the tagged state, not the head.
    let out = include_ok(&host, &["add", &url, "vendor/tagged", "--tag", "v1.0"]);
    assert!(out.contains("pinned to tag 'v1.0'"), "got: {out}");
    assert_eq!(read(&host, "vendor/tagged/f.txt"), "v1\n");
    let marker = read(&host, "vendor/tagged/.gitrepo");
    assert!(marker.contains("branch = v1.0"), "marker:\n{marker}");
    assert!(
        marker.contains(&format!("commit = {pinned_sha}")),
        "marker:\n{marker}"
    );

    // Pin to an exact commit id.
    include_ok(
        &host,
        &["add", &url, "vendor/exact", "--commit", &pinned_sha],
    );
    assert_eq!(read(&host, "vendor/exact/f.txt"), "v1\n");

    // Pulls on pinned includes are stable no-op reports.
    let out = include_ok(&host, &["pull", "vendor/tagged"]);
    assert!(out.contains("pinned to tag 'v1.0'"), "got: {out}");
    let out = include_ok(&host, &["pull", "vendor/exact"]);
    assert!(out.contains("pinned to commit"), "got: {out}");

    // Pushing to a tag or commit is impossible and says so.
    commit_file(&host, "vendor/tagged/l.txt", "l\n", "local");
    let err = include_err(&host, &["push", "vendor/tagged"]);
    assert!(err.contains("pinned to tag 'v1.0'"), "got: {err}");
    assert!(err.contains("git include switch"), "got: {err}");
    commit_file(&host, "vendor/exact/l.txt", "l\n", "local");
    let err = include_err(&host, &["push", "vendor/exact"]);
    assert!(err.contains("pinned to commit"), "got: {err}");
}

#[test]
fn switch_pins_to_tag_and_back_to_branch() {
    let env = TestEnv::new();
    let (url, up_work) = env.upstream("lib");
    upstream_commit(&up_work, "f.txt", "v1\n", "version 1");
    git_in(&up_work, &["tag", "v1.0"]);
    git_in(&up_work, &["push", "-q", "origin", "v1.0"]);
    upstream_commit(&up_work, "f.txt", "v2\n", "version 2");

    let host = env.work_repo("host");
    include_ok(&host, &["add", &url, "vendor/lib"]); // tracks main at v2
    assert_eq!(read(&host, "vendor/lib/f.txt"), "v2\n");

    // Pin to the tag: content rolls back to the tagged state.
    let out = include_ok(&host, &["switch", "vendor/lib", "v1.0"]);
    assert!(
        out.contains("Pinned 'vendor/lib' to tag 'v1.0'"),
        "got: {out}"
    );
    assert_eq!(read(&host, "vendor/lib/f.txt"), "v1\n");
    assert_clean(&host);

    // `branches` lists the tag and marks it as tracked.
    let out = include_ok(&host, &["branches", "vendor/lib"]);
    assert!(out.contains("* v1.0"), "got: {out}");
    assert!(out.contains("  main"), "got: {out}");

    // Unpin by switching back to the branch; content moves to the head
    // again and pushing works once more.
    include_ok(&host, &["switch", "vendor/lib", "main"]);
    assert_eq!(read(&host, "vendor/lib/f.txt"), "v2\n");
    commit_file(&host, "vendor/lib/l.txt", "l\n", "after unpin");
    include_ok(&host, &["push", "vendor/lib"]);
}

// --------------------------------------------------------- force pull ----

#[test]
fn pull_force_discards_local_state() {
    let env = TestEnv::new();
    let (url, up_work) = env.upstream("lib");
    upstream_commit(&up_work, "f.txt", "upstream\n", "base");
    let host = env.work_repo("host");
    include_ok(&host, &["add", &url, "vendor/lib"]);

    // Local committed change + uncommitted edit + upstream progress.
    commit_file(&host, "vendor/lib/f.txt", "local mess\n", "local mess");
    std::fs::write(host.join("vendor/lib/f.txt"), "even worse\n").unwrap();
    upstream_commit(&up_work, "f.txt", "upstream v2\n", "upstream v2");

    include_ok(&host, &["pull", "vendor/lib", "--force"]);
    assert_eq!(read(&host, "vendor/lib/f.txt"), "upstream v2\n");
    assert_clean(&host);

    // The discarded local commit is NOT pushed later: force advanced the
    // sync point.
    let s = include_ok(&host, &["status", "vendor/lib"]);
    assert!(s.contains("clean"), "got: {s}");
    let out = include_ok(&host, &["push", "vendor/lib"]);
    assert!(out.contains("no local changes"), "got: {out}");
}

#[test]
fn pull_force_resolves_a_conflicted_pull() {
    let env = TestEnv::new();
    let (url, up_work) = env.upstream("lib");
    upstream_commit(&up_work, "s.txt", "orig\n", "base");
    let host = env.work_repo("host");
    include_ok(&host, &["add", &url, "vendor/lib"]);
    commit_file(&host, "vendor/lib/s.txt", "mine\n", "mine");
    upstream_commit(&up_work, "s.txt", "theirs\n", "theirs");

    // Normal pull conflicts and suggests --force as the bail-out.
    let err = include_err(&host, &["pull", "vendor/lib"]);
    assert!(err.contains("--force"), "got: {err}");
    // Take upstream and move on.
    include_ok(&host, &["pull", "vendor/lib", "--force"]);
    assert_eq!(read(&host, "vendor/lib/s.txt"), "theirs\n");
    assert_clean(&host);
}

// --------------------------------------------------- message templates ----

#[test]
fn commit_messages_are_templatable_via_flag_and_config() {
    let env = TestEnv::new();
    let (url, up_work) = env.upstream("lib");
    let host = env.work_repo("host");

    // --message on the command line.
    include_ok(
        &host,
        &[
            "add",
            &url,
            "vendor/lib",
            "--message",
            "vendor: import {{ subdir }} at {{ short_commit }}",
        ],
    );
    let subject = git_in(&host, &["log", "-1", "--format=%s"]);
    let sha = git_in(
        &host,
        &["config", "--file", "vendor/lib/.gitrepo", "subrepo.commit"],
    );
    assert_eq!(
        subject,
        format!("vendor: import vendor/lib at {}", &sha[..7])
    );

    // include.commitTemplate in git config, with \n escapes for the body.
    git_in(
        &host,
        &[
            "config",
            "include.commitTemplate",
            "chore({{ subdir }}): {{ action }} from {{ ref }}\\n\\nupstream: {{ remote }}",
        ],
    );
    upstream_commit(&up_work, "n.txt", "n\n", "upstream work");
    include_ok(&host, &["pull", "vendor/lib"]);
    let subject = git_in(&host, &["log", "-1", "--format=%s"]);
    assert_eq!(subject, "chore(vendor/lib): pull from main");
    let body = git_in(&host, &["log", "-1", "--format=%b"]);
    assert!(body.contains(&format!("upstream: {url}")), "body: {body}");

    // Full Jinja is available: conditionals and filters.
    upstream_commit(&up_work, "n2.txt", "n\n", "more upstream work");
    include_ok(
        &host,
        &[
            "pull",
            "vendor/lib",
            "--message",
            "{% if action == 'pull' %}update{% endif %}: {{ subdir | upper }}",
        ],
    );
    let subject = git_in(&host, &["log", "-1", "--format=%s"]);
    assert_eq!(subject, "update: VENDOR/LIB");

    // A broken template (typo'd variable) degrades to the default message
    // with a warning instead of failing the sync.
    git_in(
        &host,
        &["config", "include.commitTemplate", "oops {{ subdri }}"],
    );
    upstream_commit(&up_work, "n3.txt", "n\n", "even more upstream work");
    let out = include_cmd(&host, &["pull", "vendor/lib"]);
    assert!(out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("warning:"), "got: {stderr}");
    let subject = git_in(&host, &["log", "-1", "--format=%s"]);
    assert_eq!(subject, "git include pull vendor/lib");

    // The default (no config, no flag) keeps the structured format.
    git_in(&host, &["config", "--unset", "include.commitTemplate"]);
    upstream_commit(&up_work, "n4.txt", "n\n", "final upstream work");
    include_ok(&host, &["pull", "vendor/lib"]);
    let subject = git_in(&host, &["log", "-1", "--format=%s"]);
    assert_eq!(subject, "git include pull vendor/lib");
}

// ------------------------------------------- push to a different branch ----

#[test]
fn push_to_new_remote_branch_leaves_tracking_untouched() {
    let env = TestEnv::new();
    let (url, _up) = env.upstream("lib");
    let host = env.work_repo("host");
    include_ok(&host, &["add", &url, "vendor/lib"]);
    commit_file(&host, "vendor/lib/feat.txt", "f\n", "propose a feature");

    let out = include_ok(
        &host,
        &["push", "vendor/lib", "--branch", "feature/proposal"],
    );
    assert!(out.contains("feature/proposal"), "got: {out}");
    assert!(out.contains("still tracks 'main'"), "got: {out}");

    // The feature branch exists upstream with our commit; main is untouched.
    let clone = env.path("check");
    git_in(
        env.root.path(),
        &[
            "clone",
            "-q",
            "-b",
            "feature/proposal",
            &url,
            clone.to_str().unwrap(),
        ],
    );
    assert_eq!(read(&clone, "feat.txt"), "f\n");
    let log = git_in(&clone, &["log", "--format=%s", "feature/proposal"]);
    assert!(log.contains("propose a feature"), "log: {log}");
    assert!(!git_in(&clone, &["ls-tree", "--name-only", "origin/main"]).contains("feat.txt"));

    // No marker bookkeeping happened: the commit still counts as unpushed
    // relative to the tracked branch (it lands there via the PR merge).
    let s = include_ok(&host, &["status", "vendor/lib"]);
    assert!(s.contains("1 commit(s) to push"), "got: {s}");

    // The feature branch now sits ahead of the recorded base, so pushing
    // to it again is refused (it is not at the base anymore).
    let err = include_err(&host, &["push", "vendor/lib", "-b", "feature/proposal"]);
    assert!(err.contains("already exists"), "got: {err}");
}

#[test]
fn push_to_existing_branch_not_at_base_is_refused() {
    let env = TestEnv::new();
    let (url, up_work) = env.upstream("lib");
    git_in(&up_work, &["checkout", "-q", "-b", "other"]);
    upstream_commit(&up_work, "other.txt", "o\n", "other work");
    git_in(&up_work, &["checkout", "-q", "main"]);

    let host = env.work_repo("host");
    include_ok(&host, &["add", &url, "vendor/lib", "--branch", "main"]);
    commit_file(&host, "vendor/lib/x.txt", "x\n", "local");

    let err = include_err(&host, &["push", "vendor/lib", "--branch", "other"]);
    assert!(err.contains("already exists"), "got: {err}");
}

// ------------------------------------------------------ changing remote ----

#[test]
fn remote_command_shows_and_changes_the_upstream() {
    let env = TestEnv::new();
    let (url, up_work) = env.upstream("lib");
    let host = env.work_repo("host");
    include_ok(&host, &["add", &url, "vendor/lib"]);

    // Show the current remote.
    let out = include_ok(&host, &["remote", "vendor/lib"]);
    assert_eq!(out.trim(), url);

    // Mirror the upstream to a new location and point the include at it.
    let mirror = env.path("mirror.git");
    git_in(
        env.root.path(),
        &["clone", "-q", "--mirror", &url, mirror.to_str().unwrap()],
    );
    let mirror_url = mirror.to_str().unwrap().to_string();
    include_ok(&host, &["remote", "vendor/lib", &mirror_url]);
    let recorded = git_in(
        &host,
        &["config", "--file", "vendor/lib/.gitrepo", "subrepo.remote"],
    );
    assert_eq!(recorded, mirror_url);
    assert_clean(&host);

    // Pull and push now go through the new remote.
    let mirror_work = env.path("mirror-work");
    git_in(
        env.root.path(),
        &["clone", "-q", &mirror_url, mirror_work.to_str().unwrap()],
    );
    configure_user(&mirror_work);
    commit_file(&mirror_work, "new.txt", "n\n", "work on the mirror");
    git_in(&mirror_work, &["push", "-q", "origin", "main"]);
    include_ok(&host, &["pull", "vendor/lib"]);
    assert_eq!(read(&host, "vendor/lib/new.txt"), "n\n");
    // The old upstream never saw that commit.
    assert!(!up_work.join("new.txt").exists());

    // A remote that lacks the tracked branch is refused.
    let empty = env.bare_repo("empty.git");
    let err = include_err(&host, &["remote", "vendor/lib", empty.to_str().unwrap()]);
    assert!(err.contains("does not exist"), "got: {err}");
}

// -------------------------------------------------- message ref kinds ----

#[test]
fn default_commit_message_names_the_ref_kind() {
    let env = TestEnv::new();
    let (url, up_work) = env.upstream("lib");
    upstream_commit(&up_work, "f.txt", "1\n", "one");
    git_in(&up_work, &["tag", "v1"]);
    git_in(&up_work, &["push", "-q", "origin", "v1"]);

    let host = env.work_repo("host");
    include_ok(&host, &["add", &url, "vendor/lib", "--tag", "v1"]);
    let body = git_in(&host, &["log", "-1", "--format=%B"]);
    assert!(body.contains("tag: \"v1\""), "body: {body}");

    include_ok(&host, &["switch", "vendor/lib", "main"]);
    let body = git_in(&host, &["log", "-1", "--format=%B"]);
    assert!(body.contains("branch: \"main\""), "body: {body}");

    // Flags used are visible in the subject.
    commit_file(&host, "vendor/lib/junk.txt", "j\n", "junk");
    include_ok(&host, &["pull", "vendor/lib", "--force"]);
    let subject = git_in(&host, &["log", "-1", "--format=%s"]);
    assert_eq!(subject, "git include pull --force vendor/lib");
}

// ------------------------------------------------------- init / export ----

#[test]
fn init_extracts_directory_history_and_publishes_it() {
    let env = TestEnv::new();
    let host = env.work_repo("host");

    // Grow a normal directory over several commits, including one mixed
    // commit that also touches files outside the directory.
    commit_file(&host, "mylib/core.txt", "v1\n", "mylib: create core");
    commit_file(&host, "unrelated.txt", "x\n", "unrelated work");
    std::fs::write(host.join("mylib/core.txt"), "v2\n").unwrap();
    std::fs::write(host.join("other.txt"), "o\n").unwrap();
    git_in(&host, &["add", "."]);
    git_in(
        &host,
        &["commit", "-q", "-m", "mixed: improve core and other"],
    );
    commit_file(&host, "mylib/extra.txt", "e\n", "mylib: add extra");

    // Export it to a brand-new (empty) repository.
    let bare = env.bare_repo("exported.git");
    let url = bare.to_str().unwrap().to_string();
    let out = include_ok(&host, &["init", "mylib", "--remote", &url]);
    assert!(out.contains("extracted 3 commit(s)"), "got: {out}");
    assert!(read(&host, "mylib/.gitrepo").contains(&url));
    assert_clean(&host);

    include_ok(&host, &["push", "mylib"]);

    // The published repository contains exactly the directory's history:
    // its files, its commits (original messages), nothing else.
    let clone = env.path("check");
    git_in(
        env.root.path(),
        &["clone", "-q", &url, clone.to_str().unwrap()],
    );
    assert_eq!(read(&clone, "core.txt"), "v2\n");
    assert_eq!(read(&clone, "extra.txt"), "e\n");
    assert!(!clone.join("unrelated.txt").exists());
    assert!(!clone.join("other.txt").exists());
    assert!(!clone.join(".gitrepo").exists());
    let log = git_in(&clone, &["log", "--format=%s", "main"]);
    assert_eq!(
        log,
        "mylib: add extra\nmixed: improve core and other\nmylib: create core"
    );

    // From here on it behaves like any other include.
    let s = include_ok(&host, &["status", "mylib"]);
    assert!(s.contains("up to date"), "got: {s}");
    assert!(s.contains("clean"), "got: {s}");
    commit_file(&host, "mylib/core.txt", "v3\n", "mylib: v3");
    include_ok(&host, &["push", "mylib"]);
    git_in(&clone, &["pull", "-q", "origin", "main"]);
    assert_eq!(read(&clone, "core.txt"), "v3\n");
    let out = include_ok(&host, &["pull", "mylib"]);
    assert!(out.contains("up to date"), "got: {out}");
}

#[test]
fn init_refuses_untracked_or_already_included_directories() {
    let env = TestEnv::new();
    let host = env.work_repo("host");
    let bare = env.bare_repo("x.git");
    let url = bare.to_str().unwrap().to_string();

    let err = include_err(&host, &["init", "nonexistent", "--remote", &url]);
    assert!(err.contains("no tracked files"), "got: {err}");

    let (lib_url, _up) = env.upstream("lib");
    include_ok(&host, &["add", &lib_url, "vendor/lib"]);
    let err = include_err(&host, &["init", "vendor/lib", "--remote", &url]);
    assert!(err.contains("already an included repository"), "got: {err}");
}

// ------------------------------------------------------------- remove ----

#[test]
fn remove_deletes_include_and_commits() {
    let env = TestEnv::new();
    let (url, _up) = env.upstream("lib");
    let host = env.work_repo("host");
    include_ok(&host, &["add", &url, "vendor/lib"]);

    include_ok(&host, &["remove", "vendor/lib"]);
    assert!(!host.join("vendor/lib").exists());
    assert_clean(&host);
    let out = include_ok(&host, &["list"]);
    assert!(out.contains("No included repositories"), "got: {out}");
}

// ------------------------------------------------------------ various ----

#[test]
fn commands_work_from_a_subdirectory_with_relative_paths() {
    let env = TestEnv::new();
    let (url, up_work) = env.upstream("lib");
    let host = env.work_repo("host");
    include_ok(&host, &["add", &url, "vendor/lib"]);

    upstream_commit(&up_work, "x.txt", "x\n", "x");
    // From inside vendor/, refer to the include as just "lib".
    include_ok(&host.join("vendor"), &["pull", "lib"]);
    assert_eq!(read(&host, "vendor/lib/x.txt"), "x\n");

    // And from inside the include itself, as ".".
    let s = include_ok(&host.join("vendor/lib"), &["status", "."]);
    assert!(s.contains("vendor/lib"), "got: {s}");
}

#[test]
fn pull_refuses_dirty_worktree() {
    let env = TestEnv::new();
    let (url, up_work) = env.upstream("lib");
    let host = env.work_repo("host");
    include_ok(&host, &["add", &url, "vendor/lib"]);
    upstream_commit(&up_work, "y.txt", "y\n", "y");

    std::fs::write(host.join("README.md"), "dirty\n").unwrap();
    let err = include_err(&host, &["pull", "vendor/lib"]);
    assert!(err.contains("uncommitted changes"), "got: {err}");
}

#[test]
fn errors_are_helpful_for_unknown_directories() {
    let env = TestEnv::new();
    let host = env.work_repo("host");
    let err = include_err(&host, &["pull", "does/not/exist"]);
    assert!(err.contains("not an included repository"), "got: {err}");
    assert!(err.contains("git include list"), "got: {err}");
}

#[test]
fn completions_cover_direct_and_git_subcommand_usage() {
    let env = TestEnv::new();
    let host = env.work_repo("host");

    let bash = include_ok(&host, &["completions", "bash"]);
    assert!(bash.contains("_git-include"), "clap completion missing");
    assert!(bash.contains("_git_include"), "git subcommand shim missing");
    assert!(
        bash.contains("ls-files -- '*.gitrepo'"),
        "dynamic dir completion missing"
    );

    let zsh = include_ok(&host, &["completions", "zsh"]);
    assert!(zsh.contains("#compdef git-include"), "zsh compdef missing");

    let fish = include_ok(&host, &["completions", "fish"]);
    assert!(
        fish.contains("__fish_git_include_dirs"),
        "fish shim missing"
    );
}

// ---------------------------------------------------------------- LFS ----

/// Full LFS round-trip. Skipped (with a note) when git-lfs is not
/// installed on the machine running the tests.
#[test]
fn lfs_content_is_fetched_on_add_and_pull() {
    if !lfs_available() {
        eprintln!("SKIP: git-lfs is not installed");
        return;
    }
    let env = TestEnv::new();
    let (url, up_work) = env.upstream("lib");

    git_in(&up_work, &["lfs", "install", "--local"]);
    git_in(&up_work, &["lfs", "track", "*.bin"]);
    std::fs::write(up_work.join("big.bin"), vec![7u8; 4096]).unwrap();
    git_in(&up_work, &["add", ".gitattributes", "big.bin"]);
    git_in(&up_work, &["commit", "-q", "-m", "add LFS file"]);
    git_in(&up_work, &["push", "-q", "origin", "main"]);

    let host = env.work_repo("host");
    include_ok(&host, &["add", &url, "vendor/lib"]);

    let content = std::fs::read(host.join("vendor/lib/big.bin")).unwrap();
    assert_eq!(
        content.len(),
        4096,
        "expected real LFS content, not a pointer file"
    );
    assert_eq!(content[0], 7u8);
}

fn lfs_available() -> bool {
    std::process::Command::new("git")
        .args(["lfs", "version"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}
