use sha2::{Digest, Sha256};
use std::collections::VecDeque;
use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
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

enum StubReply {
    Ok(String),
    Error,
}

fn request_body(stream: &mut std::net::TcpStream) -> String {
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut bytes = Vec::new();
    let mut chunk = [0u8; 4096];
    let mut expected = None;
    loop {
        let read = stream.read(&mut chunk).unwrap();
        if read == 0 {
            break;
        }
        bytes.extend_from_slice(&chunk[..read]);
        if expected.is_none() {
            if let Some(header_end) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
                let headers = String::from_utf8_lossy(&bytes[..header_end]);
                let content_length = headers
                    .lines()
                    .find_map(|line| {
                        line.split_once(':').and_then(|(name, value)| {
                            name.eq_ignore_ascii_case("content-length")
                                .then(|| value.trim().parse::<usize>().ok())
                                .flatten()
                        })
                    })
                    .unwrap_or(0);
                expected = Some(header_end + 4 + content_length);
            }
        }
        if expected.is_some_and(|expected| bytes.len() >= expected) {
            break;
        }
    }
    String::from_utf8(bytes).unwrap()
}

fn spawn_ollama_stub(
    replies: Vec<StubReply>,
) -> (
    String,
    Arc<Mutex<Vec<String>>>,
    thread::JoinHandle<()>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let base_url = format!("http://{}", listener.local_addr().unwrap());
    let requests = Arc::new(Mutex::new(Vec::new()));
    let recorded = Arc::clone(&requests);
    let handle = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(30);
        let mut replies = VecDeque::from(replies);
        while !replies.is_empty() && Instant::now() < deadline {
            let (mut stream, _) = match listener.accept() {
                Ok(connection) => connection,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(10));
                    continue;
                }
                Err(error) => panic!("accept scripted Ollama request: {error}"),
            };
            let request = request_body(&mut stream);
            recorded.lock().unwrap().push(request);
            let (status, body) = match replies.pop_front().unwrap() {
                StubReply::Ok(content) => (
                    "200 OK",
                    serde_json::json!({
                        "model": "test",
                        "created_at": "2026-07-23T00:00:00Z",
                        "message": {"role": "assistant", "content": content},
                        "done": true
                    })
                    .to_string(),
                ),
                StubReply::Error => (
                    "500 Internal Server Error",
                    serde_json::json!({"error": "scripted compile failure"}).to_string(),
                ),
            };
            write!(
                stream,
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
            stream.flush().unwrap();
        }
        assert!(replies.is_empty(), "not every scripted Ollama reply was used");
    });
    (base_url, requests, handle)
}

#[test]
fn injection_trace_links_canonical_project_events_and_cli_lookup() {
    let tmp = TempDir::new().unwrap();
    let home = tmp.path().join("pc-home");
    let subject = tmp.path().join("subject");
    init_subject(&subject);

    let config_output = run_pc(&home, &subject, &["config"]);
    assert!(config_output.status.success());
    let _ = project_path(&home, &subject);
    let config_path = home.join("config.json");
    let mut config: serde_json::Value =
        serde_json::from_slice(&fs::read(&config_path).unwrap()).unwrap();
    config["inject_select_model"] = serde_json::Value::String("ollama:test-select".into());
    config["inject_compile_model"] = serde_json::Value::String("ollama:test-compile".into());
    fs::write(&config_path, serde_json::to_vec_pretty(&config).unwrap()).unwrap();

    let input = format!(
        "{{\"cwd\":{},\"session_id\":\"trace-session\",\"prompt\":\"ok\"}}",
        serde_json::to_string(subject.to_str().unwrap()).unwrap()
    );
    let output = run_hook(
        &home,
        &subject,
        &["hook", "inject", "--harness", "codex"],
        &input,
        false,
    );
    assert!(output.status.success());
    assert!(output.stdout.is_empty());

    let project_uuid = git(&subject, &["config", "--local", "--get", "pc.projectUuid"]);
    let project_id = git(&subject, &["config", "--local", "--get", "pc.projectId"]);
    let traces = home
        .join("state")
        .join(&project_uuid)
        .join("logs")
        .join("inject-runs");
    let trace_path = fs::read_dir(&traces)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let run_id = trace_path.file_stem().unwrap().to_str().unwrap();
    let trace: serde_json::Value =
        serde_json::from_slice(&fs::read(&trace_path).unwrap()).unwrap();

    assert_eq!(trace["run_id"], run_id);
    assert_eq!(trace["project"]["project_id"], project_id);
    assert_eq!(trace["project"]["project_uuid"], project_uuid);
    assert_eq!(trace["hook"]["prompt_chars"], 2);
    assert!(trace["hook"]["prompt_sha256"].as_str().unwrap().len() == 64);
    assert_eq!(trace["final_outcome"]["reason"], "no_index");
    assert!(!trace.to_string().contains("\"prompt\":\"ok\""));

    let event_log = fs::read_to_string(home.join("state/events.jsonl")).unwrap();
    let event: serde_json::Value =
        serde_json::from_str(event_log.lines().next().unwrap()).unwrap();
    assert_eq!(event["req"], run_id);
    assert_eq!(event["project"], project_id);

    let inspected = run_pc(&home, &subject, &["debug", "trace", run_id]);
    assert!(inspected.status.success());
    let inspected_trace: serde_json::Value =
        serde_json::from_slice(&inspected.stdout).unwrap();
    assert_eq!(inspected_trace, trace);
}

#[test]
fn disabled_logging_creates_neither_events_nor_injection_traces() {
    let tmp = TempDir::new().unwrap();
    let home = tmp.path().join("pc-home");
    let subject = tmp.path().join("subject");
    init_subject(&subject);

    let config_output = run_pc(&home, &subject, &["config"]);
    assert!(config_output.status.success());
    let _ = project_path(&home, &subject);
    let config_path = home.join("config.json");
    let mut config: serde_json::Value =
        serde_json::from_slice(&fs::read(&config_path).unwrap()).unwrap();
    config["logging_enabled"] = serde_json::Value::Bool(false);
    fs::write(&config_path, serde_json::to_vec_pretty(&config).unwrap()).unwrap();

    let input = format!(
        "{{\"cwd\":{},\"session_id\":\"trace-disabled\",\"prompt\":\"ok\"}}",
        serde_json::to_string(subject.to_str().unwrap()).unwrap()
    );
    let output = run_hook(
        &home,
        &subject,
        &["hook", "inject", "--harness", "codex"],
        &input,
        false,
    );
    assert!(output.status.success());

    let project_uuid = git(&subject, &["config", "--local", "--get", "pc.projectUuid"]);
    assert!(!home.join("state/events.jsonl").exists());
    assert!(
        !home
            .join("state")
            .join(project_uuid)
            .join("logs")
            .join("inject-runs")
            .exists()
    );
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
fn direct_noun_hook_activation_fails_closed_at_the_subprocess_boundary() {
    let tmp = TempDir::new().unwrap();
    let home = tmp.path().join("pc-home");
    let subject = tmp.path().join("subject");
    init_subject(&subject);

    let config_output = run_pc(&home, &subject, &["config"]);
    assert!(config_output.status.success());
    let _ = project_path(&home, &subject);
    let project_uuid = git(&subject, &["config", "--local", "--get", "pc.projectUuid"]);
    let state = home.join("state").join(project_uuid);
    let wiki = state.join("wiki");
    fs::create_dir_all(wiki.join("guides")).unwrap();
    fs::create_dir_all(wiki.join("nouns")).unwrap();
    fs::write(
        wiki.join("guides/purplepages.md"),
        "---\n\
title: PurplePages\n\
slug: purplepages\n\
topic: product\n\
summary: The project's public directory.\n\
---\n\n\
# PurplePages\n\n\
The project's public directory.\n",
    )
    .unwrap();
    fs::write(
        wiki.join("_index.md"),
        "# Project Knowledge Index\n\n\
| Slug | Title | Summary | Tags | Volatility | Verified | Topic |\n\
|---|---|---|---|---|---|---|\n\
| [purplepages](guides/purplepages.md) | PurplePages | The project's public directory. |  | warm | 2026-07-23 | product |\n",
    )
    .unwrap();
    fs::write(
        wiki.join("nouns/realness.jsonl"),
        "{\"canonical\":\"purple page\",\"name\":\"PurplePages\",\"signed\":3,\"status\":\"real\"}\n",
    )
    .unwrap();
    // The file only needs to exist to pass the no-index guard. The deliberately unsupported
    // embed provider below fails deterministically before opening it or making a network call.
    fs::write(state.join("index.db"), "").unwrap();

    let config_path = home.join("config.json");
    let mut config: serde_json::Value =
        serde_json::from_slice(&fs::read(&config_path).unwrap()).unwrap();
    config["inject_select_model"] = serde_json::Value::String("ollama:test-select".into());
    config["inject_compile_model"] = serde_json::Value::String("ollama:test-compile".into());
    config["ollama_base_url"] = serde_json::Value::String("http://127.0.0.1:9".into());
    config["embed_provider"] =
        serde_json::Value::String("deterministic-test-failure".into());
    fs::write(&config_path, serde_json::to_vec_pretty(&config).unwrap()).unwrap();

    let transcript = tmp.path().join("transcript.jsonl");
    fs::write(
        &transcript,
        "{\"role\":\"user\",\"content\":\"We already discussed PurplePages.\"}\n",
    )
    .unwrap();
    let input = |with_recent: bool| {
        let mut value = serde_json::json!({
            "cwd": subject,
            "session_id": "noun-failure",
            "prompt": "what is PurplePages?"
        });
        if with_recent {
            value["transcript_path"] =
                serde_json::Value::String(transcript.to_string_lossy().to_string());
        }
        serde_json::to_string(&value).unwrap()
    };

    for with_recent in [false, true] {
        let output = run_hook(
            &home,
            &subject,
            &["hook", "inject", "--harness", "codex"],
            &input(with_recent),
            false,
        );
        assert!(output.status.success());
        assert!(
            output.stdout.is_empty(),
            "provider failure must emit no hook payload: {}",
            String::from_utf8_lossy(&output.stdout)
        );
    }

    let traces = fs::read_dir(state.join("logs/inject-runs"))
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| {
            serde_json::from_slice::<serde_json::Value>(&fs::read(entry.path()).unwrap()).unwrap()
        })
        .filter(|trace| trace["hook"]["session_id"] == "noun-failure")
        .collect::<Vec<_>>();
    assert_eq!(traces.len(), 2);
    for trace in traces {
        assert_eq!(trace["final_outcome"]["reason"], "retrieval_failed");
        assert!(
            trace["events"]
                .as_array()
                .unwrap()
                .iter()
                .any(|event| event["event"] == "inject.start"),
            "the direct three-word noun query must pass the trivial-prompt gate"
        );
        assert!(trace.get("delivery").is_none());
        assert!(trace.get("ledger").is_none());
    }
    assert!(!state.join("ledger").exists());
    assert!(!state.join("noun-ledger").exists());
}

#[test]
fn noun_alias_hook_uses_real_guide_dedups_and_fails_closed_with_scripted_ollama() {
    let tmp = TempDir::new().unwrap();
    let home = tmp.path().join("pc-home");
    let subject = tmp.path().join("subject");
    init_subject(&subject);
    assert!(run_pc(&home, &subject, &["config"]).status.success());
    let _ = project_path(&home, &subject);
    let project_uuid = git(&subject, &["config", "--local", "--get", "pc.projectUuid"]);
    let state = home.join("state").join(project_uuid);
    let wiki = state.join("wiki");
    let guide = wiki.join("guides/aster.md");
    fs::create_dir_all(guide.parent().unwrap()).unwrap();
    fs::create_dir_all(wiki.join("nouns")).unwrap();
    fs::write(
        &guide,
        "---\n\
title: Aster\n\
slug: aster\n\
topic: product\n\
summary: Aster coordinates signed envelopes.\n\
---\n\n\
# Aster\n\n\
Aster coordinates signed envelopes.\n",
    )
    .unwrap();
    fs::write(
        wiki.join("_index.md"),
        "# Project Knowledge Index\n\n\
| Slug | Title | Summary | Tags | Volatility | Verified | Topic |\n\
|---|---|---|---|---|---|---|\n\
| [aster](guides/aster.md) | Aster | Aster coordinates signed envelopes. |  | warm | 2026-07-23 | product |\n",
    )
    .unwrap();
    fs::write(
        wiki.join("nouns/realness.jsonl"),
        "{\"canonical\":\"aster\",\"name\":\"Aster\",\"signed\":3,\"status\":\"real\"}\n",
    )
    .unwrap();

    let index_source = tmp.path().join("index-src/pc-memory/guides");
    fs::create_dir_all(&index_source).unwrap();
    fs::copy(&guide, index_source.join("aster.md")).unwrap();
    let index_root = tmp.path().join("index-src");
    let index_output = run_pc(
        &home,
        &subject,
        &[
            "index-files",
            "--dir",
            index_root.to_str().unwrap(),
            "--index-db",
            state.join("index.db").to_str().unwrap(),
        ],
    );
    assert!(
        index_output.status.success(),
        "index fixture: {}",
        String::from_utf8_lossy(&index_output.stderr)
    );

    let fact_line = 10;
    let compile = format!(
        "TITLE: Aster grounding\nAster coordinates signed envelopes. ({}:{fact_line})",
        guide.display()
    );
    let (ollama_url, requests, server) = spawn_ollama_stub(vec![
        StubReply::Ok("QUERY: What is Aster?\nnoun:aster".to_string()),
        StubReply::Ok(compile.clone()),
        StubReply::Ok("QUERY: What is Aster?\nnoun:aster".to_string()),
        StubReply::Ok(compile),
        StubReply::Ok("QUERY: What is Aster?\nnoun:aster".to_string()),
        StubReply::Error,
    ]);
    let config_path = home.join("config.json");
    let mut config: serde_json::Value =
        serde_json::from_slice(&fs::read(&config_path).unwrap()).unwrap();
    config["inject_select_model"] = serde_json::Value::String("ollama:test-select".into());
    config["inject_compile_model"] = serde_json::Value::String("ollama:test-compile".into());
    config["ollama_base_url"] = serde_json::Value::String(ollama_url);
    config["inject_min_prompt_words"] = serde_json::Value::from(4);
    config["inject_context_turns"] = serde_json::Value::from(0);
    config["inject_rerank"] = serde_json::Value::Bool(false);
    config["inject_browse_timeout_ms"] = serde_json::Value::from(10_000);
    fs::write(&config_path, serde_json::to_vec_pretty(&config).unwrap()).unwrap();

    let input = |session: &str| {
        serde_json::json!({
            "cwd": subject,
            "session_id": session,
            "prompt": "what is Aster?"
        })
        .to_string()
    };
    let first = run_hook(
        &home,
        &subject,
        &["hook", "inject", "--harness", "claude"],
        &input("noun-success"),
        false,
    );
    assert!(first.status.success());
    let first_stdout = String::from_utf8(first.stdout).unwrap();
    assert!(first_stdout.contains("<relevant-context from=\"pc skill\">"));
    assert!(first_stdout.contains("Aster coordinates signed envelopes."));

    let ledger = state.join("ledger/noun-success.jsonl");
    let ledger_after_first = fs::read(&ledger).unwrap();
    assert_eq!(
        String::from_utf8_lossy(&ledger_after_first).lines().count(),
        1
    );
    let repeated = run_hook(
        &home,
        &subject,
        &["hook", "inject", "--harness", "claude"],
        &input("noun-success"),
        false,
    );
    assert!(repeated.status.success());
    assert!(repeated.stdout.is_empty());
    assert_eq!(fs::read(&ledger).unwrap(), ledger_after_first);

    let failed = run_hook(
        &home,
        &subject,
        &["hook", "inject", "--harness", "claude"],
        &input("noun-failure"),
        false,
    );
    assert!(failed.status.success());
    assert!(failed.stdout.is_empty());
    assert!(!state.join("ledger/noun-failure.jsonl").exists());
    server.join().unwrap();

    let traces = fs::read_dir(state.join("logs/inject-runs"))
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| {
            serde_json::from_slice::<serde_json::Value>(&fs::read(entry.path()).unwrap()).unwrap()
        })
        .collect::<Vec<_>>();
    let session_traces = |session: &str| {
        traces
            .iter()
            .filter(|trace| trace["hook"]["session_id"] == session)
            .collect::<Vec<_>>()
    };
    let successes = session_traces("noun-success");
    assert_eq!(successes.len(), 2);
    assert!(successes.iter().any(|trace| {
        trace["final_outcome"]["outcome"] == "compiled" && trace.get("delivery").is_some()
    }));
    assert!(successes.iter().any(|trace| {
        trace["final_outcome"]["reason"] == "already_delivered"
            && trace.get("delivery").is_none()
    }));
    let failure = session_traces("noun-failure");
    assert_eq!(failure.len(), 1);
    assert_eq!(failure[0]["final_outcome"]["reason"], "provider_error");
    assert!(failure[0].get("delivery").is_none());
    assert!(failure[0].get("ledger").is_none());

    for trace in successes.into_iter().chain(failure) {
        let noun_map = trace["decisions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|decision| decision["stage"] == "noun_source_map")
            .expect("persisted noun source mapping");
        let source = &noun_map["value"]["sources"][0];
        assert_eq!(source["catalog_key"], "noun:aster");
        assert_eq!(source["source_key"], "aster");
        assert_eq!(source["currentness"], "current");
        assert!(source["score"].as_f64().unwrap() >= 0.25);
    }
    let selected_map = traces
        .iter()
        .flat_map(|trace| trace["decisions"].as_array().unwrap())
        .find(|decision| {
            decision["stage"] == "selected_source_map"
                && decision["value"]["sources"][0]["catalog_key"] == "noun:aster"
        })
        .expect("persisted selected noun mapping");
    assert_eq!(
        selected_map["value"]["sources"][0]["source_key"],
        "aster"
    );

    let requests = requests.lock().unwrap();
    assert_eq!(requests.len(), 6);
    for (index, request) in requests.iter().enumerate() {
        let expected_model = if index % 2 == 0 {
            "test-select"
        } else {
            "test-compile"
        };
        assert!(request.contains(&format!("\"model\":\"{expected_model}\"")));
        if index % 2 == 0 {
            assert!(request.contains("noun:aster"));
        } else {
            assert!(request.contains(&guide.to_string_lossy().to_string()));
            assert!(request.contains("kind=\\\"current-guide\\\""));
            assert!(!request.contains("realness.jsonl"));
            assert!(!request.contains("unresolved-noun-alias"));
        }
    }
    assert!(!state.join("noun-ledger").exists());
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
