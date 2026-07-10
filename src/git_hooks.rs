//! Retired `pc install --git-hooks` support.
//!
//! The old hook auto-committed `docs/wiki` changes after user commits. Keep the
//! status/uninstall path so existing managed hooks can be removed cleanly, but
//! do not install new auto-commit hooks.

use crate::harness::install::{strip_sentinel, write_with_parents, SENTINEL_OPEN};
use anyhow::{anyhow, Result};
use colored::Colorize;
use std::path::{Path, PathBuf};
use std::process::Command;

pub struct GitHooksOpts {
    pub dry_run: bool,
    pub status: bool,
    pub uninstall: bool,
}

pub fn run(opts: GitHooksOpts) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let dir = hooks_dir(&cwd)?;
    let path = dir.join("post-commit");

    if opts.status {
        print_status(&path);
        return Ok(());
    }

    if opts.uninstall {
        match uninstall(&path) {
            Ok(summary) => println!("{} {}", "✓".green(), summary),
            Err(e) => println!("{} {}", "✗".red(), e),
        }
        return Ok(());
    }

    println!(
        "{} pc-managed git auto-commit hooks are retired; installation is no longer supported.",
        "!".yellow()
    );
    println!(
        "  run {} to remove an existing managed hook",
        "`pc install --git-hooks --uninstall`".bold()
    );
    if opts.dry_run {
        println!("  {}", "(dry run — nothing was written)".dimmed());
    }
    Ok(())
}

/// Resolve `<git-common-dir>/hooks` for the repo containing `base`, shelling
/// out with `-C` so this is correct from any linked worktree.
fn hooks_dir(base: &Path) -> Result<PathBuf> {
    let out = Command::new("git")
        .args(["-C", &base.to_string_lossy(), "rev-parse", "--git-common-dir"])
        .output()
        .map_err(|e| anyhow!("failed to run git: {e}"))?;
    if !out.status.success() {
        return Err(anyhow!("not a git repository (or any parent up to mount point)"));
    }
    let common_dir = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let common_dir = PathBuf::from(common_dir);
    let common_dir = if common_dir.is_absolute() {
        common_dir
    } else {
        base.join(common_dir)
    };
    Ok(common_dir.join("hooks"))
}

fn uninstall(path: &Path) -> Result<String> {
    let Ok(existing) = std::fs::read_to_string(path) else {
        return Ok("nothing to remove".into());
    };
    if !existing.contains(SENTINEL_OPEN) {
        return Ok("nothing to remove (not pc-managed)".into());
    }
    let stripped = strip_sentinel(&existing);
    if stripped.trim().is_empty() || stripped.trim() == "#!/bin/sh" {
        std::fs::remove_file(path)?;
        return Ok("hook removed".into());
    }
    write_with_parents(path, &format!("{}\n", stripped.trim_end()))?;
    Ok("managed block removed (foreign hook content preserved)".into())
}

fn print_status(path: &Path) {
    if !path.exists() {
        println!("{} no post-commit hook at {}", "○".dimmed(), path.display());
        return;
    }
    let text = std::fs::read_to_string(path).unwrap_or_default();
    if text.contains(SENTINEL_OPEN) {
        println!("{} pc-managed post-commit hook installed at {}", "✓".green(), path.display());
    } else {
        println!(
            "{} a post-commit hook exists at {} but is not pc-managed",
            "!".yellow(),
            path.display()
        );
    }
}
