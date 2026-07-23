use sha2::{Digest, Sha256};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use tempfile::TempDir;

fn pc() -> &'static str {
    env!("CARGO_BIN_EXE_pc")
}

fn git(path: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn init_subject(path: &Path) {
    fs::create_dir_all(path).unwrap();
    git(path, &["init", "--initial-branch", "master"]);
    fs::write(path.join("README.md"), "subject\n").unwrap();
    git(path, &["add", "README.md"]);
    git(
        path,
        &[
            "-c",
            "user.name=test",
            "-c",
            "user.email=test@example.com",
            "commit",
            "-m",
            "initialize",
        ],
    );
}

fn run_pc(home: &Path, dir: &Path, args: &[&str]) -> Output {
    Command::new(pc())
        .env("PC_HOME", home)
        .arg("--dir")
        .arg(dir)
        .args(args)
        .output()
        .unwrap()
}

fn run_hook(home: &Path, dir: &Path, args: &[&str], input: &str, disabled: bool) -> Output {
    let mut command = Command::new(pc());
    command
        .env("PC_HOME", home)
        .current_dir(dir)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if disabled {
        command.env("PC_DISABLE_HOOKS", "1");
    }
    let mut child = command.spawn().unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

fn project_path(home: &Path, subject: &Path) -> PathBuf {
    let output = run_pc(home, subject, &["project", "path"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    PathBuf::from(String::from_utf8(output.stdout).unwrap().trim())
}

#[test]
fn non_git_hooks_are_exact_silent_noops_before_config_or_logging() {
    let tmp = TempDir::new().unwrap();
    let home = tmp.path().join("pc-home");
    let non_git = tmp.path().join("plain");
    fs::create_dir(&non_git).unwrap();
    let input = format!(
        "{{\"cwd\":{},\"session_id\":\"s\",\"prompt\":\"hello project context\",\"transcript_path\":\"/missing\"}}",
        serde_json::to_string(non_git.to_str().unwrap()).unwrap()
    );
    for args in [
        &["hook", "inject"][..],
        &["hook", "capture"][..],
        &["hook", "statusline"][..],
        &["hook", "session-start"][..],
    ] {
        let output = run_hook(&home, &non_git, args, &input, false);
        assert!(output.status.success());
        assert!(
            output.stdout.is_empty(),
            "stdout from {:?}: {:?}",
            args,
            output.stdout
        );
        assert!(
            output.stderr.is_empty(),
            "stderr from {:?}: {:?}",
            args,
            output.stderr
        );
        assert!(!home.exists());
    }

    let bare = tmp.path().join("bare.git");
    fs::create_dir(&bare).unwrap();
    git(&bare, &["init", "--bare", "--initial-branch", "master"]);
    let bare_input = format!(
        "{{\"cwd\":{},\"session_id\":\"s\",\"prompt\":\"hello\"}}",
        serde_json::to_string(bare.to_str().unwrap()).unwrap()
    );
    for args in [
        &["hook", "inject"][..],
        &["hook", "capture"][..],
        &["hook", "statusline"][..],
        &["hook", "session-start"][..],
    ] {
        let output = run_hook(&home, &bare, args, &bare_input, false);
        assert!(output.status.success());
        assert!(output.stdout.is_empty());
        assert!(output.stderr.is_empty());
        assert!(!home.exists());
    }
}

#[test]
fn missing_generation_config_warns_once_per_session_without_context() {
    let tmp = TempDir::new().unwrap();
    let home = tmp.path().join("pc-home");
    let subject = tmp.path().join("subject");
    init_subject(&subject);

    let input_for = |session: &str| {
        format!(
            "{{\"cwd\":{},\"session_id\":{},\"prompt\":\"explain the project architecture\"}}",
            serde_json::to_string(subject.to_str().unwrap()).unwrap(),
            serde_json::to_string(session).unwrap()
        )
    };

    let first = run_hook(
        &home,
        &subject,
        &["hook", "inject"],
        &input_for("session-a"),
        false,
    );
    assert!(first.status.success());
    let first_json: serde_json::Value = serde_json::from_slice(&first.stdout).unwrap();
    assert_eq!(first_json["systemMessage"], "pc: no generation config.");
    assert!(first_json.get("hookSpecificOutput").is_none());
    assert!(first_json.get("context").is_none());

    let repeated = run_hook(
        &home,
        &subject,
        &["hook", "inject"],
        &input_for("session-a"),
        false,
    );
    assert!(repeated.status.success());
    assert!(repeated.stdout.is_empty());

    let next_session = run_hook(
        &home,
        &subject,
        &["hook", "inject"],
        &input_for("session-b"),
        false,
    );
    assert!(next_session.status.success());
    let next_json: serde_json::Value = serde_json::from_slice(&next_session.stdout).unwrap();
    assert_eq!(next_json["systemMessage"], "pc: no generation config.");
    assert!(next_json.get("hookSpecificOutput").is_none());
}

#[test]
fn worktrees_share_a_store_and_same_named_repositories_do_not() {
    let tmp = TempDir::new().unwrap();
    let home = tmp.path().join("pc-home");
    let main = tmp.path().join("one").join("same");
    let other = tmp.path().join("two").join("same");
    let linked = tmp.path().join("linked");
    init_subject(&main);
    init_subject(&other);
    git(
        &main,
        &["worktree", "add", linked.to_str().unwrap(), "-b", "linked"],
    );

    let main_store = project_path(&home, &main);
    let linked_store = project_path(&home, &linked);
    let other_store = project_path(&home, &other);
    assert_eq!(main_store, linked_store);
    assert_ne!(main_store, other_store);
    assert_eq!(main_store.file_name().unwrap(), "same");
    assert_eq!(other_store.file_name().unwrap(), "same-1");
    assert_eq!(
        git(&main_store, &["rev-parse", "--is-inside-work-tree"]),
        "true"
    );
    assert!(!main.join("docs/wiki").exists());
    assert!(!other.join("docs/wiki").exists());
}

#[test]
fn a_new_store_uses_the_preconfigured_sync_branch() {
    let tmp = TempDir::new().unwrap();
    let home = tmp.path().join("pc-home");
    let subject = tmp.path().join("subject");
    init_subject(&subject);
    fs::create_dir_all(&home).unwrap();
    fs::write(home.join("config.json"), "{\"store_branch\":\"main\"}\n").unwrap();

    let store = project_path(&home, &subject);
    assert_eq!(git(&store, &["symbolic-ref", "--short", "HEAD"]), "main");
}

#[test]
fn disabled_hooks_cannot_capture_the_project_store_itself() {
    let tmp = TempDir::new().unwrap();
    let home = tmp.path().join("pc-home");
    let subject = tmp.path().join("subject");
    init_subject(&subject);
    let store = project_path(&home, &subject);
    let before = fs::read_dir(home.join("projects")).unwrap().count();
    let input = format!(
        "{{\"cwd\":{},\"session_id\":\"reconcile\"}}",
        serde_json::to_string(store.to_str().unwrap()).unwrap()
    );
    let output = run_hook(&home, &store, &["hook", "statusline"], &input, true);
    assert!(output.status.success());
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
    assert_eq!(fs::read_dir(home.join("projects")).unwrap().count(), before);

    // The detached worker repeats subject eligibility and rejects a portable
    // store even if it is invoked manually without the reconciliation flag.
    let output = run_hook(
        &home,
        &store,
        &["hook", "capture", "--deferred", "missing-capture"],
        "",
        false,
    );
    assert!(output.status.success());
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
    assert_eq!(fs::read_dir(home.join("projects")).unwrap().count(), before);
}

#[test]
fn capture_is_inboxed_even_when_store_has_an_unfinished_git_operation() {
    let tmp = TempDir::new().unwrap();
    let home = tmp.path().join("pc-home");
    let subject = tmp.path().join("subject");
    init_subject(&subject);
    let store = project_path(&home, &subject);
    fs::write(
        store.join(".git/MERGE_HEAD"),
        "0000000000000000000000000000000000000000\n",
    )
    .unwrap();
    let transcript = tmp.path().join("transcript.jsonl");
    fs::write(
        &transcript,
        "{\"role\":\"user\",\"content\":\"capture this durable decision\"}\n",
    )
    .unwrap();
    let input = format!(
        "{{\"cwd\":{},\"session_id\":\"session\",\"transcript_path\":{}}}",
        serde_json::to_string(subject.to_str().unwrap()).unwrap(),
        serde_json::to_string(transcript.to_str().unwrap()).unwrap()
    );
    let output = run_hook(&home, &subject, &["hook", "capture"], &input, false);
    assert!(output.status.success());
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());

    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(store.join("pc-project.json")).unwrap()).unwrap();
    let state = home
        .join("state")
        .join(manifest["project_uuid"].as_str().unwrap());
    let inbox_entries: Vec<_> = fs::read_dir(state.join("capture-inbox"))
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| entry.path().is_dir())
        .collect();
    assert_eq!(inbox_entries.len(), 1);
    assert!(inbox_entries[0].path().join("request.json").exists());
    assert!(inbox_entries[0].path().join("transcript.jsonl").exists());
}

#[test]
fn a_missing_bound_checkout_is_not_reinitialized_but_capture_is_still_inboxed() {
    let tmp = TempDir::new().unwrap();
    let home = tmp.path().join("pc-home");
    let subject = tmp.path().join("subject");
    init_subject(&subject);
    let store = project_path(&home, &subject);
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(store.join("pc-project.json")).unwrap()).unwrap();
    let project_uuid = manifest["project_uuid"].as_str().unwrap().to_string();
    fs::remove_dir_all(&store).unwrap();

    let output = run_pc(&home, &subject, &["project", "path"]);
    assert!(!output.status.success());
    assert!(
        !store.exists(),
        "ensure must not fabricate replacement history"
    );

    let transcript = tmp.path().join("transcript.jsonl");
    fs::write(
        &transcript,
        "{\"role\":\"user\",\"content\":\"retain this capture while the store is restored\"}\n",
    )
    .unwrap();
    let input = format!(
        "{{\"cwd\":{},\"session_id\":\"missing-store\",\"transcript_path\":{}}}",
        serde_json::to_string(subject.to_str().unwrap()).unwrap(),
        serde_json::to_string(transcript.to_str().unwrap()).unwrap()
    );
    let output = run_hook(&home, &subject, &["hook", "capture"], &input, false);
    assert!(output.status.success());
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
    assert!(!store.exists());
    let inbox = home.join("state").join(project_uuid).join("capture-inbox");
    assert_eq!(
        fs::read_dir(inbox)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.path().is_dir())
            .count(),
        1
    );
}

#[test]
fn attaching_an_existing_store_materializes_its_latest_capture() {
    let tmp = TempDir::new().unwrap();
    let home = tmp.path().join("pc-home");
    let original_subject = tmp.path().join("original-subject");
    let attached_subject = tmp.path().join("attached-subject");
    init_subject(&original_subject);
    init_subject(&attached_subject);
    let store = project_path(&home, &original_subject);
    let store_manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(store.join("pc-project.json")).unwrap()).unwrap();
    let project_uuid = store_manifest["project_uuid"].as_str().unwrap();

    let content = b"# Shared decision\n\nPortable context follows the repository.\n";
    let hash = format!("{:x}", Sha256::digest(content));
    let object = store.join("objects").join(&hash[..2]).join(&hash);
    fs::create_dir_all(object.parent().unwrap()).unwrap();
    fs::write(&object, content).unwrap();
    let capture_id = "cross-machine-capture";
    let capture_dir = store.join("captures").join(capture_id);
    fs::create_dir_all(&capture_dir).unwrap();
    let manifest = serde_json::json!({
        "schema_version": 1,
        "project_uuid": project_uuid,
        "capture_id": capture_id,
        "parent_capture_id": null,
        "harness": "test",
        "session_id": "portable-session",
        "transcript_sha256": hash,
        "files": { "guides/shared-decision.md": hash }
    });
    fs::write(
        capture_dir.join("manifest.json"),
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();
    git(&store, &["add", "objects", "captures"]);
    git(
        &store,
        &[
            "-c",
            "user.name=test",
            "-c",
            "user.email=test@example.com",
            "commit",
            "-m",
            "portable capture",
            "-m",
            "PC-Capture-Id: cross-machine-capture",
        ],
    );

    let store_arg = store.to_str().unwrap();
    let output = run_pc(&home, &attached_subject, &["project", "attach", store_arg]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read(
            home.join("state")
                .join(project_uuid)
                .join("wiki/guides/shared-decision.md")
        )
        .unwrap(),
        content
    );
    assert_eq!(
        git(
            &attached_subject,
            &["config", "--local", "--get", "pc.projectUuid"]
        ),
        project_uuid
    );
}

#[test]
fn failed_attach_validation_does_not_mutate_subject_binding() {
    let tmp = TempDir::new().unwrap();
    let home = tmp.path().join("pc-home");
    let original_subject = tmp.path().join("original-subject");
    let target_subject = tmp.path().join("target-subject");
    init_subject(&original_subject);
    init_subject(&target_subject);
    let store = project_path(&home, &original_subject);
    let store_manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(store.join("pc-project.json")).unwrap()).unwrap();
    let project_uuid = store_manifest["project_uuid"].as_str().unwrap();
    let capture_dir = store.join("captures/rewritten-capture");
    fs::create_dir_all(&capture_dir).unwrap();
    let mut manifest = serde_json::json!({
        "schema_version": 1,
        "project_uuid": project_uuid,
        "capture_id": "rewritten-capture",
        "parent_capture_id": null,
        "harness": "test",
        "session_id": "first-session",
        "transcript_sha256": "00",
        "files": {}
    });
    let path = capture_dir.join("manifest.json");
    fs::write(&path, serde_json::to_vec_pretty(&manifest).unwrap()).unwrap();
    git(&store, &["add", "captures"]);
    git(
        &store,
        &[
            "-c",
            "user.name=test",
            "-c",
            "user.email=test@example.com",
            "commit",
            "-m",
            "add capture",
        ],
    );
    manifest["session_id"] = serde_json::Value::String("rewritten-session".into());
    fs::write(&path, serde_json::to_vec_pretty(&manifest).unwrap()).unwrap();
    git(&store, &["add", "captures"]);
    git(
        &store,
        &[
            "-c",
            "user.name=test",
            "-c",
            "user.email=test@example.com",
            "commit",
            "-m",
            "rewrite immutable capture",
        ],
    );

    let output = run_pc(
        &home,
        &target_subject,
        &["project", "attach", store.to_str().unwrap()],
    );
    assert!(!output.status.success());
    let binding = Command::new("git")
        .arg("-C")
        .arg(&target_subject)
        .args(["config", "--local", "--get", "pc.projectUuid"])
        .output()
        .unwrap();
    assert_eq!(binding.status.code(), Some(1));
    assert!(binding.stdout.is_empty());
}

#[test]
fn legacy_application_home_is_ignored_without_migration() {
    let tmp = TempDir::new().unwrap();
    let fake_home = tmp.path().join("home");
    let subject = tmp.path().join("subject");
    init_subject(&subject);
    fs::create_dir_all(fake_home.join(".proactive-context")).unwrap();
    let legacy_config = fake_home.join(".proactive-context/config.json");
    fs::write(
        &legacy_config,
        "this legacy file must not be read or rewritten\n",
    )
    .unwrap();

    let output = Command::new(pc())
        .env_remove("PC_HOME")
        .env("HOME", &fake_home)
        .arg("--dir")
        .arg(&subject)
        .args(["config", "show"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(fake_home.join(".pc/config.json").is_file());
    assert_eq!(
        fs::read_to_string(legacy_config).unwrap(),
        "this legacy file must not be read or rewritten\n"
    );
}
