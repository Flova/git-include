use std::io::{IsTerminal, Write};

use anyhow::{Context, Result};
use git2::{DiffFormat, DiffStatsFormat, Oid};

use crate::git::Git;
use crate::ops::Include;

/// `git include diff <dir>`: local changes since the last sync.
/// `--upstream`: compare against the latest upstream head instead.
pub fn run(git: &Git, subdir: &str, upstream: bool, fetch: bool, stat: bool) -> Result<()> {
    let inc = Include::load(git, subdir)?;

    if fetch {
        eprintln!("Fetching {} ({}) ...", inc.meta.remote, inc.meta.branch);
        git.fetch_rev(&inc.meta.remote, &inc.meta.branch, None, &inc.pin_ref())?;
    }

    let local = inc.local_tree_stripped()?;

    let against = if upstream {
        let head = inc.pinned_upstream().with_context(|| {
            format!(
                "no upstream state known yet; run `git include diff {subdir} --upstream --fetch`"
            )
        })?;
        git.rev_parse(&format!("{head}^{{tree}}"))
            .context("upstream commit has no tree")?
    } else {
        inc.ensure_base_commit()?;
        inc.base_tree().context("base commit has no tree")?
    };

    if against == local {
        if upstream {
            println!("'{subdir}' is identical to upstream {}.", inc.meta.branch);
        } else {
            println!("'{subdir}' has no local changes since the last sync.");
        }
        return Ok(());
    }

    let old = git.repo.find_tree(Oid::from_str(&against)?)?;
    let new = git.repo.find_tree(Oid::from_str(&local)?)?;
    let diff = git.repo.diff_tree_to_tree(Some(&old), Some(&new), None)?;

    // Colorize like `git diff` when writing to a terminal (NO_COLOR
    // opts out, https://no-color.org/).
    let color = std::io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none();
    let paint = |origin: char| -> (&'static str, &'static str) {
        if !color {
            return ("", "");
        }
        match origin {
            '+' => ("\x1b[32m", "\x1b[m"), // additions: green
            '-' => ("\x1b[31m", "\x1b[m"), // deletions: red
            'H' => ("\x1b[36m", "\x1b[m"), // hunk headers: cyan
            'F' => ("\x1b[1m", "\x1b[m"),  // file headers: bold
            _ => ("", ""),
        }
    };

    let mut stdout = std::io::stdout().lock();
    if stat {
        let stats = diff.stats()?;
        let buf = stats.to_buf(DiffStatsFormat::FULL, 80)?;
        stdout.write_all(&buf)?;
    } else {
        diff.print(DiffFormat::Patch, |_delta, _hunk, line| {
            let origin = line.origin();
            let (on, off) = paint(origin);
            let content = String::from_utf8_lossy(line.content());
            let (body, newline) = match content.strip_suffix('\n') {
                Some(body) => (body, "\n"),
                None => (content.as_ref(), ""),
            };
            let prefix = if matches!(origin, '+' | '-' | ' ') {
                origin.to_string()
            } else {
                String::new()
            };
            write!(stdout, "{on}{prefix}{body}{off}{newline}").is_ok()
        })?;
    }
    Ok(())
}
