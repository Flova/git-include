//! A full user journey through git-include, as one continuous story:
//! vendoring a library, everyday development, syncing both ways, pinning
//! to a release, recovering with a force pull, customizing messages,
//! exporting a homegrown directory, and cleaning up. Every stage asserts
//! the exact repository state a real user would observe.

mod common;

use common::*;

#[test]
fn full_user_journey() {
    let env = TestEnv::new();

    // ---- Scene 1: an upstream library exists, with a tagged release. ----
    let (lib_url, lib) = env.upstream("widgets");
    upstream_commit(
        &lib,
        "src/core.txt",
        "core v1\n",
        "core: initial implementation",
    );
    upstream_commit(&lib, "docs/usage.txt", "how to use\n", "docs: usage");
    git_in(&lib, &["tag", "-a", "v1.0", "-m", "widgets 1.0"]);
    git_in(&lib, &["push", "-q", "origin", "v1.0"]);

    // ---- Scene 2: we vendor it into our product repository. -------------
    let host = env.work_repo("product");
    include_ok(&host, &["add", &lib_url, "vendor/widgets"]);
    assert_eq!(read(&host, "vendor/widgets/src/core.txt"), "core v1\n");
    assert_clean(&host);
    let out = include_ok(&host, &["list"]);
    assert!(out.contains("vendor/widgets"), "got: {out}");

    // ---- Scene 3: everyday development, mixed with vendored fixes. ------
    commit_file(&host, "src/app.txt", "our app\n", "app: skeleton");
    commit_file(
        &host,
        "vendor/widgets/src/core.txt",
        "core v1 + our fix\n",
        "widgets: fix crash on empty input",
    );
    commit_file(&host, "src/app.txt", "our app v2\n", "app: use widgets");

    let s = include_ok(&host, &["status"]);
    assert!(s.contains("1 commit(s) to push"), "got: {s}");
    let d = include_ok(&host, &["diff", "vendor/widgets"]);
    assert!(d.contains("+core v1 + our fix"), "got: {d}");

    // ---- Scene 4: upstream keeps moving; we pull before contributing. ---
    upstream_commit(&lib, "src/extra.txt", "extra\n", "core: add extra module");
    let s = include_ok(&host, &["status", "--fetch"]);
    assert!(s.contains("1 new commit(s) available"), "got: {s}");
    include_ok(&host, &["pull", "vendor/widgets"]);
    assert_eq!(read(&host, "vendor/widgets/src/extra.txt"), "extra\n");
    assert_eq!(
        read(&host, "vendor/widgets/src/core.txt"),
        "core v1 + our fix\n",
        "local fix must survive the pull"
    );

    // ---- Scene 5: contribute our fix back, as our own commit. -----------
    include_ok(&host, &["push", "vendor/widgets"]);
    git_in(&lib, &["pull", "-q", "origin", "main"]);
    let log = git_in(&lib, &["log", "--format=%s", "main"]);
    assert!(
        log.contains("widgets: fix crash on empty input"),
        "log: {log}"
    );
    assert!(
        !log.contains("git include"),
        "no sync noise upstream: {log}"
    );
    assert_eq!(read(&lib, "src/core.txt"), "core v1 + our fix\n");
    let s = include_ok(&host, &["status"]);
    assert!(s.contains("up to date"), "got: {s}");
    assert!(s.contains("clean"), "got: {s}");

    // ---- Scene 6: a release freeze — pin the vendored lib to v1.0. ------
    let out = include_ok(&host, &["branches", "vendor/widgets"]);
    assert!(out.contains("v1.0"), "got: {out}");
    include_ok(&host, &["switch", "vendor/widgets", "v1.0", "--force"]);
    assert_eq!(read(&host, "vendor/widgets/src/core.txt"), "core v1\n");
    assert!(!host.join("vendor/widgets/src/extra.txt").exists());
    let out = include_ok(&host, &["pull", "vendor/widgets"]);
    assert!(out.contains("pinned to tag 'v1.0'"), "got: {out}");

    // ---- Scene 7: freeze over — back to main, everything returns. -------
    include_ok(&host, &["switch", "vendor/widgets", "main"]);
    assert_eq!(read(&host, "vendor/widgets/src/extra.txt"), "extra\n");
    assert_eq!(
        read(&host, "vendor/widgets/src/core.txt"),
        "core v1 + our fix\n"
    );
    assert_clean(&host);

    // ---- Scene 8: an experiment goes wrong; force pull resets it. -------
    commit_file(
        &host,
        "vendor/widgets/src/core.txt",
        "broken experiment\n",
        "widgets: experiment (do not push!)",
    );
    std::fs::write(host.join("vendor/widgets/src/core.txt"), "worse\n").unwrap();
    include_ok(&host, &["pull", "vendor/widgets", "--force"]);
    assert_eq!(
        read(&host, "vendor/widgets/src/core.txt"),
        "core v1 + our fix\n"
    );
    assert_clean(&host);
    let out = include_ok(&host, &["push", "vendor/widgets"]);
    assert!(
        out.contains("no local changes"),
        "the experiment must not leak: {out}"
    );

    // ---- Scene 9: the team standardizes sync commit messages. -----------
    git_in(
        &host,
        &[
            "config",
            "include.commitTemplate",
            "chore(vendor): {{ action }} {{ subdir }} @ {{ short_commit }}",
        ],
    );
    upstream_commit(
        &lib,
        "src/core.txt",
        "core v1 + our fix + more\n",
        "core: more work",
    );
    include_ok(&host, &["pull", "vendor/widgets"]);
    let subject = git_in(&host, &["log", "-1", "--format=%s"]);
    assert!(
        subject.starts_with("chore(vendor): pull vendor/widgets @ "),
        "got: {subject}"
    );

    // ---- Scene 10: our own 'utils' folder grows up into a library. ------
    commit_file(
        &host,
        "utils/strings.txt",
        "trim\n",
        "utils: string helpers",
    );
    commit_file(
        &host,
        "utils/strings.txt",
        "trim, split\n",
        "utils: add split",
    );
    let bare = env.bare_repo("utils.git");
    let utils_url = bare.to_str().unwrap().to_string();
    include_ok(&host, &["init", "utils", "--remote", &utils_url]);
    include_ok(&host, &["push", "utils"]);

    let check = env.path("utils-check");
    git_in(
        env.root.path(),
        &["clone", "-q", &utils_url, check.to_str().unwrap()],
    );
    assert_eq!(read(&check, "strings.txt"), "trim, split\n");
    let log = git_in(&check, &["log", "--format=%s", "main"]);
    assert_eq!(log, "utils: add split\nutils: string helpers");

    // Both includes now show up, and everything is in sync.
    let out = include_ok(&host, &["list"]);
    assert!(out.contains("utils"), "got: {out}");
    assert!(out.contains("vendor/widgets"), "got: {out}");
    let s = include_ok(&host, &["status"]);
    assert_eq!(s.matches("upstream: up to date").count(), 2, "got: {s}");
    assert_eq!(s.matches("local:    clean").count(), 2, "got: {s}");

    // ---- Scene 11: a teammate clones the product repo and joins in. -----
    let (product_url, product_work) = env.upstream("product-origin");
    // (publish our product repo so the teammate can clone it)
    git_in(&host, &["remote", "add", "origin", &product_url]);
    git_in(&host, &["push", "-q", "origin", "main", "--force"]);
    drop(product_work);
    let teammate = env.path("teammate");
    git_in(
        env.root.path(),
        &["clone", "-q", &product_url, teammate.to_str().unwrap()],
    );
    configure_user(&teammate);
    assert_eq!(read(&teammate, "vendor/widgets/src/extra.txt"), "extra\n");
    commit_file(
        &teammate,
        "vendor/widgets/src/extra.txt",
        "extra improved\n",
        "widgets: improve extra module",
    );
    include_ok(&teammate, &["push", "vendor/widgets"]);
    git_in(&lib, &["pull", "-q", "origin", "main"]);
    assert_eq!(read(&lib, "src/extra.txt"), "extra improved\n");

    // ---- Scene 12: the vendored lib is retired. --------------------------
    include_ok(&host, &["remove", "vendor/widgets"]);
    assert!(!host.join("vendor/widgets").exists());
    let out = include_ok(&host, &["list"]);
    assert!(!out.contains("vendor/widgets"), "got: {out}");
    assert!(out.contains("utils"), "the exported include stays: {out}");
    assert_clean(&host);
}
