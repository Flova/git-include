use anyhow::Result;

use crate::git::Git;
use crate::gitrepo::MARKER_FILE;
use crate::ops::{Include, find_all_includes};
use crate::util::short;

pub fn run(git: &Git, subdir: Option<&str>, fetch: bool) -> Result<()> {
    let targets: Vec<String> = match subdir {
        Some(s) => vec![s.to_string()],
        None => {
            let all = find_all_includes(git)?;
            if all.is_empty() {
                println!("No included repositories. Use `git include add <remote> <dir>`.");
                return Ok(());
            }
            all
        }
    };

    for (i, dir) in targets.iter().enumerate() {
        if i > 0 {
            println!();
        }
        print_one(git, dir, fetch)?;
    }
    Ok(())
}

fn print_one(git: &Git, subdir: &str, fetch: bool) -> Result<()> {
    let inc = Include::load(git, subdir)?;
    println!("{subdir}");
    println!("  remote:   {}", inc.meta.remote);
    println!(
        "  branch:   {} (synced at {})",
        inc.meta.branch,
        short(&inc.meta.commit)
    );

    if fetch {
        eprintln!("  (fetching {} ...)", inc.meta.remote);
        let _ = git.fetch_branch(&inc.meta.remote, &inc.meta.branch, &inc.pin_ref());
    }

    // Upstream side: commits we have not pulled yet.
    match inc.pinned_upstream() {
        Some(upstream) if git.has_commit(&inc.meta.commit) => {
            if upstream == inc.meta.commit {
                println!("  upstream: up to date");
            } else {
                let behind = git
                    .count_range(&inc.meta.commit, &upstream)
                    .map(|n| n.to_string())
                    .unwrap_or_else(|_| "?".into());
                println!(
                    "  upstream: {behind} new commit(s) available -> `git include pull {subdir}`"
                );
            }
        }
        _ => println!("  upstream: unknown (run `git include status {subdir} --fetch`)"),
    }

    // Local side: unpushed commits and uncommitted edits.
    match (inc.local_tree_stripped(), inc.base_tree()) {
        (Ok(local), Some(base)) => {
            let unpushed = count_unpushed(&inc).unwrap_or(0);
            if local == base && unpushed == 0 {
                println!("  local:    clean");
            } else if unpushed > 0 {
                println!("  local:    {unpushed} commit(s) to push -> `git include push {subdir}`");
            } else {
                println!("  local:    modified since last sync (`git include diff {subdir}`)");
            }
        }
        _ => println!("  local:    unknown (upstream base commit not available locally)"),
    }
    if git.subdir_has_uncommitted(subdir) {
        println!("  worktree: uncommitted changes in '{subdir}' (see `git status`)");
    }
    Ok(())
}

/// Number of local commits whose content would be replayed by `push`
/// (mirrors push's skip rules, so the count matches what push will do).
pub fn count_unpushed(inc: &Include<'_>) -> Result<usize> {
    let git = inc.git;
    let Some(parent) = inc.meta.parent.as_deref() else {
        return Ok(0);
    };
    if !git.has_commit(&inc.meta.commit) || !git.has_commit(parent) {
        return Ok(0);
    }
    let mut tip_tree = git
        .rev_parse(&format!("{}^{{tree}}", inc.meta.commit))
        .unwrap_or_default();
    let mut count = 0usize;
    for commit in git.walk_range(parent, &git.head()?)? {
        let Some(tree) = git.tree_at(&commit, &inc.subdir) else {
            continue;
        };
        let stripped = git.tree_without_entry(&tree, MARKER_FILE)?;
        if stripped != tip_tree {
            count += 1;
            tip_tree = stripped;
        }
    }
    Ok(count)
}
