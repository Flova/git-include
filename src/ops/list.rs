use anyhow::Result;

use crate::git::Git;
use crate::ops::{Include, find_all_includes};
use crate::util::short;

/// `git include list`: every included repository (nested ones indented
/// under the include that contains them).
pub fn run(git: &Git) -> Result<()> {
    let dirs = find_all_includes(git)?;
    if dirs.is_empty() {
        println!("No included repositories. Use `git include add <remote> <dir>`.");
        return Ok(());
    }

    for dir in &dirs {
        // Nesting depth = number of other includes that are ancestors.
        let depth = dirs
            .iter()
            .filter(|other| *other != dir && dir.starts_with(&format!("{other}/")))
            .count();
        let indent = "  ".repeat(depth);
        match Include::load(git, dir) {
            Ok(inc) => println!(
                "{indent}{dir}  <-  {} ({} @ {})",
                inc.meta.remote,
                inc.meta.branch,
                short(&inc.meta.commit)
            ),
            Err(_) => println!("{indent}{dir}  <-  (unreadable .gitrepo)"),
        }
    }
    Ok(())
}
