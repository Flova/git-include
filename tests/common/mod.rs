//! Test harness: builds real git repositories in temp directories and runs
//! the compiled git-include binary against them.
// Shared by several test binaries; not every binary uses every helper.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tempfile::TempDir;

pub struct TestEnv {
    pub root: TempDir,
}

impl TestEnv {
    pub fn new() -> Self {
        TestEnv {
            root: TempDir::new().expect("create temp dir"),
        }
    }

    pub fn path(&self, name: &str) -> PathBuf {
        self.root.path().join(name)
    }

    /// Create a bare repository (acts as "the server" for an upstream).
    pub fn bare_repo(&self, name: &str) -> PathBuf {
        let path = self.path(name);
        git_in(
            self.root.path(),
            &["init", "--bare", "-b", "main", path.to_str().unwrap()],
        );
        path
    }

    /// Create a working repository with an initial commit.
    pub fn work_repo(&self, name: &str) -> PathBuf {
        let path = self.path(name);
        std::fs::create_dir_all(&path).unwrap();
        git_in(&path, &["init", "-b", "main"]);
        configure_user(&path);
        std::fs::write(path.join("README.md"), format!("# {name}\n")).unwrap();
        git_in(&path, &["add", "."]);
        git_in(&path, &["commit", "-q", "-m", "initial commit"]);
        path
    }

    /// Create an upstream: a bare "server" repo plus a working clone used
    /// to add commits to it. Returns (bare_url, workdir).
    pub fn upstream(&self, name: &str) -> (String, PathBuf) {
        let bare = self.bare_repo(&format!("{name}.git"));
        let work = self.work_repo(&format!("{name}-work"));
        git_in(&work, &["remote", "add", "origin", bare.to_str().unwrap()]);
        git_in(&work, &["push", "-q", "origin", "main"]);
        (bare.to_str().unwrap().to_string(), work)
    }
}

pub fn configure_user(repo: &Path) {
    git_in(repo, &["config", "user.name", "Test User"]);
    git_in(repo, &["config", "user.email", "test@example.com"]);
}

/// Run git in `dir`, panicking on failure (tests assert via git state).
pub fn git_in(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .current_dir(dir)
        .args(args)
        .output()
        .expect("spawn git");
    if !out.status.success() {
        panic!(
            "git {:?} failed in {}:\n{}",
            args,
            dir.display(),
            String::from_utf8_lossy(&out.stderr)
        );
    }
    String::from_utf8_lossy(&out.stdout).trim_end().to_string()
}

/// Add (or overwrite) a file and commit it.
pub fn commit_file(repo: &Path, file: &str, content: &str, message: &str) {
    let path = repo.join(file);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(&path, content).unwrap();
    git_in(repo, &["add", "--", file]);
    git_in(repo, &["commit", "-q", "-m", message]);
}

/// Commit in an upstream workdir and push to its bare "server".
pub fn upstream_commit(work: &Path, file: &str, content: &str, message: &str) {
    commit_file(work, file, content, message);
    git_in(work, &["push", "-q", "origin", "HEAD"]);
}

/// Run the compiled git-include binary in `dir` with `args`.
pub fn include_cmd(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_git-include"))
        .current_dir(dir)
        .args(args)
        .output()
        .expect("spawn git-include")
}

/// Run git-include and require success, returning stdout.
pub fn include_ok(dir: &Path, args: &[&str]) -> String {
    let out = include_cmd(dir, args);
    assert!(
        out.status.success(),
        "git-include {:?} failed:\nstdout: {}\nstderr: {}",
        args,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).to_string()
}

/// Run git-include and require failure, returning combined output.
pub fn include_err(dir: &Path, args: &[&str]) -> String {
    let out = include_cmd(dir, args);
    assert!(
        !out.status.success(),
        "git-include {:?} unexpectedly succeeded:\n{}",
        args,
        String::from_utf8_lossy(&out.stdout)
    );
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

pub fn read(repo: &Path, file: &str) -> String {
    std::fs::read_to_string(repo.join(file)).unwrap_or_else(|e| panic!("cannot read {file}: {e}"))
}

/// The worktree must be pristine after every successful command.
pub fn assert_clean(repo: &Path) {
    let status = git_in(repo, &["status", "--porcelain"]);
    assert!(
        status.is_empty(),
        "worktree of {} is not clean:\n{status}",
        repo.display()
    );
}
