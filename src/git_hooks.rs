//! `pc install --git-hooks` — install a git `post-commit` hook that auto-commits
//! `docs/wiki` changes the capture pipeline writes out-of-band (after a session
//! ends, independent of the user's own commit cadence).

use crate::harness::install::{strip_sentinel, write_with_parents, SENTINEL_CLOSE, SENTINEL_OPEN};
use anyhow::{anyhow, Result};
use colored::Colorize;
use std::path::{Path, PathBuf};
use std::process::Command;

const HOOK_BODY: &str = r#"if [ -z "$PC_GIT_HOOKS_ACTIVE" ]; then
  export PC_GIT_HOOKS_ACTIVE=1
  if [ -n "$(git status --porcelain -- docs/wiki 2>/dev/null)" ]; then
    git add docs/wiki >/dev/null 2>&1
    if ! git diff --cached --quiet -- docs/wiki 2>/dev/null; then
      git commit --no-verify -m "docs(wiki): auto-update captured knowledge" -- docs/wiki >/dev/null 2>&1
    fi
  fi
fi
"#;

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

    println!("{} git post-commit hook ({})", "Installing".bold(), path.display());
    match install(&path, opts.dry_run) {
        Ok(summary) => {
            println!("  {} {}", "✓".green(), summary);
            if !opts.dry_run {
                println!(
                    "  {} pending docs/wiki changes will be committed as a follow-up commit after each `git commit`",
                    "note:".yellow()
                );
            }
        }
        Err(e) => println!("  {} {}", "✗".red(), e),
    }
    if opts.dry_run {
        println!("\n{}", "(dry run — nothing was written)".dimmed());
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

/// True if `root`'s repo already has our managed block in `post-commit`.
pub fn is_installed(root: &Path) -> bool {
    let Ok(dir) = hooks_dir(root) else { return false };
    std::fs::read_to_string(dir.join("post-commit"))
        .map(|s| s.contains(SENTINEL_OPEN))
        .unwrap_or(false)
}

fn install(path: &Path, dry: bool) -> Result<String> {
    let block = format!("{}\n{}{}", SENTINEL_OPEN, HOOK_BODY, SENTINEL_CLOSE);
    let existing = std::fs::read_to_string(path).unwrap_or_default();
    let already_managed = existing.contains(SENTINEL_OPEN);
    let stripped = strip_sentinel(&existing);
    let base = if stripped.trim().is_empty() { "#!/bin/sh".to_string() } else { stripped };
    let sep = if base.trim().is_empty() { "" } else { "\n" };
    let next = format!("{}{}{}\n", base.trim_end(), sep, block);

    if dry {
        return Ok(format!("would write {}:\n{}", path.display(), indent(&block)));
    }
    write_with_parents(path, &next)?;
    make_executable(path)?;
    Ok(if already_managed { "hook updated".to_string() } else { "hook installed".to_string() })
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

#[cfg(unix)]
fn make_executable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(path)?.permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(path, perms)?;
    Ok(())
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) -> Result<()> {
    Ok(())
}

fn indent(s: &str) -> String {
    s.lines().map(|l| format!("    {l}")).collect::<Vec<_>>().join("\n")
}
