use anyhow::{anyhow, Result};
use std::path::Path;
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutoCommitOutcome {
    Committed(String),
    NoChanges,
    Skipped(String),
}

pub fn commit_docs_wiki(project_root: &Path) -> Result<AutoCommitOutcome> {
    if std::env::var("PC_CAPTURE_AUTO_COMMIT_WIKI").as_deref() == Ok("0") {
        return Ok(AutoCommitOutcome::Skipped(
            "disabled by PC_CAPTURE_AUTO_COMMIT_WIKI=0".to_string(),
        ));
    }
    if !project_root.join("docs").join("wiki").is_dir() {
        return Ok(AutoCommitOutcome::Skipped(
            "docs/wiki is missing".to_string(),
        ));
    }
    if !git_ok(project_root, &["rev-parse", "--is-inside-work-tree"])? {
        return Ok(AutoCommitOutcome::Skipped(
            "not a git work tree".to_string(),
        ));
    }
    if let Some(op) = active_git_operation(project_root)? {
        return Ok(AutoCommitOutcome::Skipped(format!("git {op} in progress")));
    }
    if git_output(project_root, &["status", "--porcelain", "--", "docs/wiki"])?
        .trim()
        .is_empty()
    {
        return Ok(AutoCommitOutcome::NoChanges);
    }

    git_success(project_root, &["add", "docs/wiki"])?;
    if git_ok(
        project_root,
        &["diff", "--cached", "--quiet", "--", "docs/wiki"],
    )? {
        return Ok(AutoCommitOutcome::NoChanges);
    }
    git_success_env(
        project_root,
        &[
            "commit",
            "--no-verify",
            "-m",
            "docs(wiki): auto-update captured knowledge",
            "--",
            "docs/wiki",
        ],
        &[
            ("PC_GIT_HOOKS_ACTIVE", "1"),
            ("PC_CAPTURE_AUTO_COMMIT_ACTIVE", "1"),
        ],
    )?;
    let head = git_output(project_root, &["rev-parse", "--short", "HEAD"])?;
    Ok(AutoCommitOutcome::Committed(head.trim().to_string()))
}

fn active_git_operation(project_root: &Path) -> Result<Option<&'static str>> {
    for (path, name) in [
        ("MERGE_HEAD", "merge"),
        ("CHERRY_PICK_HEAD", "cherry-pick"),
        ("REVERT_HEAD", "revert"),
        ("rebase-merge", "rebase"),
        ("rebase-apply", "rebase"),
    ] {
        let git_path = git_output(project_root, &["rev-parse", "--git-path", path])?;
        if project_root.join(git_path.trim()).exists() || Path::new(git_path.trim()).exists() {
            return Ok(Some(name));
        }
    }
    Ok(None)
}

fn git_output(project_root: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(project_root)
        .args(args)
        .output()?;
    if !output.status.success() {
        return Err(anyhow!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn git_ok(project_root: &Path, args: &[&str]) -> Result<bool> {
    let output = Command::new("git")
        .arg("-C")
        .arg(project_root)
        .args(args)
        .output()?;
    Ok(output.status.success())
}

fn git_success(project_root: &Path, args: &[&str]) -> Result<()> {
    git_success_env(project_root, args, &[])
}

fn git_success_env(project_root: &Path, args: &[&str], envs: &[(&str, &str)]) -> Result<()> {
    let mut cmd = Command::new("git");
    cmd.arg("-C").arg(project_root).args(args);
    for (key, value) in envs {
        cmd.env(key, value);
    }
    let output = cmd.output()?;
    if !output.status.success() {
        return Err(anyhow!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn init_repo() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        git_success(tmp.path(), &["init"]).unwrap();
        git_success(tmp.path(), &["config", "user.email", "pc@example.invalid"]).unwrap();
        git_success(tmp.path(), &["config", "user.name", "proactive-context"]).unwrap();
        tmp
    }

    #[test]
    fn commits_only_docs_wiki_changes() {
        let tmp = init_repo();
        fs::create_dir_all(tmp.path().join("docs/wiki")).unwrap();
        fs::write(tmp.path().join("README.md"), "draft\n").unwrap();
        fs::write(tmp.path().join("docs/wiki/guide.md"), "# Guide\n").unwrap();

        let outcome = commit_docs_wiki(tmp.path()).unwrap();
        assert!(matches!(outcome, AutoCommitOutcome::Committed(_)));

        let files = git_output(
            tmp.path(),
            &["show", "--name-only", "--pretty=format:", "HEAD"],
        )
        .unwrap();
        assert!(files.contains("docs/wiki/guide.md"));
        assert!(!files.contains("README.md"));
        assert_eq!(
            git_output(tmp.path(), &["status", "--porcelain", "--", "README.md"])
                .unwrap()
                .trim(),
            "?? README.md"
        );
    }

    #[test]
    fn no_changes_when_docs_wiki_is_clean() {
        let tmp = init_repo();
        fs::create_dir_all(tmp.path().join("docs/wiki")).unwrap();
        assert_eq!(
            commit_docs_wiki(tmp.path()).unwrap(),
            AutoCommitOutcome::NoChanges
        );
    }
}
